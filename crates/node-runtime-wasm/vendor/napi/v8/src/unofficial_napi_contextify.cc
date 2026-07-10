#include "unofficial_napi.h"

#include <algorithm>
#include <array>
#include <cerrno>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdlib>
#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>
#include <string_view>
#include <thread>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#if !defined(_WIN32)
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <fcntl.h>
#endif
#endif

#include "internal/napi_v8_env.h"
#include "internal/napi_module_wrap_record.h"
#include "internal/unofficial_napi_bridge.h"
#include "node_api.h"
#include "unofficial_napi_error_utils.h"

namespace {

constexpr int kHostDefinedOptionsId = 8;
constexpr int kHostDefinedOptionsLength = 9;

struct ContextRecord {
  napi_ref key_ref = nullptr;
  v8::Global<v8::Context> context;
  std::unique_ptr<v8::MicrotaskQueue> own_microtask_queue;
};

std::mutex g_context_mu;
std::unordered_map<napi_env, std::vector<ContextRecord>> g_context_records;
std::unordered_set<napi_env> g_context_cleanup_hooks;

struct SavedOwnProperty {
  v8::Global<v8::Name> key;
  v8::Global<v8::Value> value;
};

}  // namespace

namespace v8impl::detail {
template <>
struct napi_lifetime_type_name__<ModuleWrapRecord> {
  static constexpr const char* value = "module_wrap_record";
};
}  // namespace v8impl::detail

namespace {

using v8impl::detail::ModuleImportAttributeRecord;
using v8impl::detail::ModuleRequestRecord;
using v8impl::detail::ModuleWrapRecord;

struct ModuleWrapBindingState {
  napi_ref import_module_dynamically_ref = nullptr;
  napi_ref initialize_import_meta_ref = nullptr;
  std::vector<ModuleWrapRecord*> modules;
  ModuleWrapRecord* temporary_required_module_facade_original = nullptr;
};

napi_value GetSymbolsBindingProperty(napi_env env, const char* property_name);
napi_value GetSourceTextModuleDefaultHdoSymbol(napi_env env);

std::mutex g_module_wrap_mu;
std::unordered_map<napi_env, ModuleWrapBindingState> g_module_wrap_states;
std::unordered_set<napi_env> g_module_wrap_cleanup_hooks;

v8::Local<v8::String> OneByteString(v8::Isolate* isolate, const char* value) {
  return v8::String::NewFromUtf8(isolate, value, v8::NewStringType::kInternalized)
      .ToLocalChecked();
}

static const auto kEsmSyntaxErrorMessages = std::array<std::string_view, 3>{
    "Cannot use import statement outside a module",
    "Unexpected token 'export'",
    "Cannot use 'import.meta' outside a module"};

static const auto kThrowsOnlyInCjsErrorMessages = std::array<std::string_view, 6>{
    "Identifier 'module' has already been declared",
    "Identifier 'exports' has already been declared",
    "Identifier 'require' has already been declared",
    "Identifier '__filename' has already been declared",
    "Identifier '__dirname' has already been declared",
    "await is only valid in async functions and the top level bodies of modules"};

static const auto kMaybeTopLevelAwaitErrors = std::array<std::string_view, 2>{
    "missing ) after argument list",
    "SyntaxError: Unexpected"};

bool IsNullish(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

napi_status ThrowTryCatchException(napi_env env, v8::TryCatch& try_catch) {
  if (env == nullptr || !try_catch.HasCaught() || try_catch.HasTerminated()) {
    return napi_pending_exception;
  }
  v8::Local<v8::Value> exception = try_catch.Exception();
  if (!exception.IsEmpty()) {
    napi_value wrapped = napi_v8_wrap_value(env, exception);
    if (wrapped != nullptr) {
      napi_status status = napi_throw(env, wrapped);
      return status == napi_ok ? napi_pending_exception : status;
    }
  }
  try_catch.ReThrow();
  return napi_pending_exception;
}

bool CoerceToStringValue(napi_env env, napi_value input, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (input == nullptr) return false;
  return napi_coerce_to_string(env, input, out) == napi_ok && *out != nullptr;
}

v8::Local<v8::String> ToV8String(napi_env env, napi_value value, const char* fallback) {
  v8::Isolate* isolate = env->isolate;
  napi_value str = nullptr;
  if (!CoerceToStringValue(env, value, &str)) {
    return v8::String::NewFromUtf8(isolate, fallback, v8::NewStringType::kNormal).ToLocalChecked();
  }
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(str);
  if (raw.IsEmpty() || !raw->IsString()) {
    return v8::String::NewFromUtf8(isolate, fallback, v8::NewStringType::kNormal).ToLocalChecked();
  }
  return raw.As<v8::String>();
}

std::string ToUtf8String(napi_env env, napi_value value, const char* fallback) {
  v8::Isolate* isolate = env->isolate;
  napi_value str = nullptr;
  if (!CoerceToStringValue(env, value, &str)) {
    return fallback == nullptr ? "" : fallback;
  }
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(str);
  if (raw.IsEmpty()) {
    return fallback == nullptr ? "" : fallback;
  }
  v8::String::Utf8Value utf8(isolate, raw);
  if (*utf8 == nullptr) {
    return fallback == nullptr ? "" : fallback;
  }
  return std::string(*utf8, static_cast<size_t>(utf8.length()));
}

bool StartsWithBomHashbang(const std::string& source) {
  return source.size() >= 5 &&
         static_cast<unsigned char>(source[0]) == 0xef &&
         static_cast<unsigned char>(source[1]) == 0xbb &&
         static_cast<unsigned char>(source[2]) == 0xbf &&
         source[3] == '#' && source[4] == '!';
}

bool SetNamed(v8::Local<v8::Context> context,
              v8::Local<v8::Object> target,
              const char* key,
              v8::Local<v8::Value> value) {
  return target->Set(context, OneByteString(context->GetIsolate(), key), value).FromMaybe(false);
}

bool SetSymbol(v8::Local<v8::Context> context,
               v8::Local<v8::Object> target,
               v8::Local<v8::Symbol> key,
               v8::Local<v8::Value> value) {
  if (key.IsEmpty()) return false;
  return target->Set(context, key, value).FromMaybe(false);
}

v8::Local<v8::Private> ApiPrivate(v8::Isolate* isolate, const char* description) {
  return v8::Private::ForApi(isolate, OneByteString(isolate, description));
}

bool SetApiPrivate(v8::Local<v8::Context> context,
                   v8::Local<v8::Object> target,
                   const char* description,
                   v8::Local<v8::Value> value) {
  if (target.IsEmpty() || description == nullptr) return false;
  return target->SetPrivate(context, ApiPrivate(context->GetIsolate(), description), value).FromMaybe(false);
}

void ResetRef(napi_env env, napi_ref* ref_ptr) {
  if (env == nullptr || ref_ptr == nullptr || *ref_ptr == nullptr) return;
  napi_delete_reference(env, *ref_ptr);
  *ref_ptr = nullptr;
}

enum class SignalPropagation {
  kContinuePropagation,
  kStopPropagation,
};

#if !defined(_WIN32)
#if defined(__APPLE__)
struct SignalSemaphore {
  sem_t* handle = SEM_FAILED;
};
#else
struct SignalSemaphore {
  sem_t handle{};
};
#endif

bool InitSignalSemaphore(SignalSemaphore* semaphore) {
  if (semaphore == nullptr) return false;
#if defined(__APPLE__)
  std::string name = "/edgejs-sigint-" + std::to_string(static_cast<long long>(getpid())) + "-" +
                     std::to_string(reinterpret_cast<uintptr_t>(semaphore));
  sem_t* handle = sem_open(name.c_str(), O_CREAT | O_EXCL, 0600, 0);
  if (handle == SEM_FAILED) return false;
  semaphore->handle = handle;
  (void)sem_unlink(name.c_str());
  return true;
#else
  return sem_init(&semaphore->handle, 0, 0) == 0;
#endif
}

void DestroySignalSemaphore(SignalSemaphore* semaphore) {
  if (semaphore == nullptr) return;
#if defined(__APPLE__)
  if (semaphore->handle != SEM_FAILED) {
    (void)sem_close(semaphore->handle);
    semaphore->handle = SEM_FAILED;
  }
#else
  (void)sem_destroy(&semaphore->handle);
#endif
}

bool WaitSignalSemaphore(SignalSemaphore* semaphore) {
  if (semaphore == nullptr) return false;
  for (;;) {
#if defined(__APPLE__)
    if (semaphore->handle == SEM_FAILED) return false;
    if (sem_wait(semaphore->handle) == 0) return true;
#else
    if (sem_wait(&semaphore->handle) == 0) return true;
#endif
    if (errno != EINTR) return false;
  }
}

bool PostSignalSemaphore(SignalSemaphore* semaphore) {
  if (semaphore == nullptr) return false;
#if defined(__APPLE__)
  return semaphore->handle != SEM_FAILED && sem_post(semaphore->handle) == 0;
#else
  return sem_post(&semaphore->handle) == 0;
#endif
}
#endif

class Watchdog {
 public:
  Watchdog(v8::Isolate* isolate, uint64_t timeout_ms, bool* timed_out)
      : isolate_(isolate), timed_out_(timed_out) {
    thread_ = std::thread([this, timeout_ms]() {
      std::unique_lock<std::mutex> lock(mutex_);
      const bool cancelled = cv_.wait_for(
          lock,
          std::chrono::milliseconds(timeout_ms),
          [this]() { return stop_requested_; });
      if (cancelled) return;
      if (timed_out_ != nullptr) {
        *timed_out_ = true;
      }
      if (isolate_ != nullptr) {
        isolate_->TerminateExecution();
      }
    });
  }

  ~Watchdog() {
    {
      std::lock_guard<std::mutex> lock(mutex_);
      stop_requested_ = true;
    }
    cv_.notify_one();
    if (thread_.joinable()) {
      thread_.join();
    }
  }

 private:
  v8::Isolate* isolate_ = nullptr;
  bool* timed_out_ = nullptr;
  std::mutex mutex_;
  std::condition_variable cv_;
  bool stop_requested_ = false;
  std::thread thread_;
};

class SigintWatchdogBase {
 public:
  virtual ~SigintWatchdogBase() = default;
  virtual SignalPropagation HandleSigint() = 0;
};

class SigintWatchdogHelper {
 public:
  static SigintWatchdogHelper& GetInstance() {
    static SigintWatchdogHelper instance;
    return instance;
  }

  static std::mutex& GetActionMutex() {
    static std::mutex action_mutex;
    return action_mutex;
  }

  void Register(SigintWatchdogBase* watchdog) {
    std::lock_guard<std::mutex> lock(list_mutex_);
    watchdogs_.push_back(watchdog);
  }

  void Unregister(SigintWatchdogBase* watchdog) {
    std::lock_guard<std::mutex> lock(list_mutex_);
    auto it = std::find(watchdogs_.begin(), watchdogs_.end(), watchdog);
    if (it != watchdogs_.end()) {
      watchdogs_.erase(it);
    }
  }

  bool HasPendingSignal() {
    std::lock_guard<std::mutex> lock(list_mutex_);
    return has_pending_signal_;
  }

  int Start() {
    std::lock_guard<std::mutex> lock(mutex_);
    if (start_stop_count_++ > 0) {
      return 0;
    }

#if !defined(_WIN32)
    if (!sem_initialized_) {
      if (!InitSignalSemaphore(&sem_)) {
        start_stop_count_--;
        return -1;
      }
      sem_initialized_ = true;
    }

    {
      std::lock_guard<std::mutex> list_lock(list_mutex_);
      has_pending_signal_ = false;
      stopping_ = false;
    }

    sigset_t sigmask;
    sigfillset(&sigmask);
    sigset_t savemask;
    if (pthread_sigmask(SIG_SETMASK, &sigmask, &savemask) != 0) {
      start_stop_count_--;
      return -1;
    }
    sigmask = savemask;

    const int rc = pthread_create(&thread_, nullptr, &SigintWatchdogHelper::Run, this);
    const int restore_rc = pthread_sigmask(SIG_SETMASK, &sigmask, nullptr);
    if (restore_rc != 0) {
      start_stop_count_--;
      return -1;
    }
    if (rc != 0) {
      start_stop_count_--;
      return rc;
    }
    has_running_thread_ = true;

    struct sigaction action;
    std::memset(&action, 0, sizeof(action));
    sigfillset(&action.sa_mask);
    action.sa_sigaction = &SigintWatchdogHelper::HandleSignal;
    action.sa_flags = SA_SIGINFO;
    if (sigaction(SIGINT, &action, &previous_sigint_action_) != 0) {
      (void)PostSignalSemaphore(&sem_);
      (void)pthread_join(thread_, nullptr);
      has_running_thread_ = false;
      start_stop_count_--;
      return -1;
    }
    has_previous_sigint_action_ = true;
#else
    has_pending_signal_ = false;
#endif

    return 0;
  }

  bool Stop() {
    std::lock_guard<std::mutex> lock(mutex_);
    if (start_stop_count_ <= 0) {
      has_pending_signal_ = false;
      return false;
    }

    bool had_pending_signal;
    {
      std::lock_guard<std::mutex> list_lock(list_mutex_);
      had_pending_signal = has_pending_signal_;
      if (--start_stop_count_ > 0) {
        has_pending_signal_ = false;
        return had_pending_signal;
      }
#if !defined(_WIN32)
      stopping_ = true;
#endif
      watchdogs_.clear();
    }

#if !defined(_WIN32)
    if (has_running_thread_) {
      (void)PostSignalSemaphore(&sem_);
      (void)pthread_join(thread_, nullptr);
      has_running_thread_ = false;
    }
    if (has_previous_sigint_action_) {
      (void)sigaction(SIGINT, &previous_sigint_action_, nullptr);
      has_previous_sigint_action_ = false;
    }
#endif

    had_pending_signal = has_pending_signal_;
    has_pending_signal_ = false;
    return had_pending_signal;
  }

 private:
  SigintWatchdogHelper() = default;

  ~SigintWatchdogHelper() {
#if !defined(_WIN32)
    {
      std::lock_guard<std::mutex> lock(mutex_);
      start_stop_count_ = 1;
    }
    (void)Stop();
    if (sem_initialized_) {
      DestroySignalSemaphore(&sem_);
    }
#endif
  }

  static bool InformWatchdogsAboutSignal() {
    SigintWatchdogHelper& helper = GetInstance();

    std::lock_guard<std::mutex> list_lock(helper.list_mutex_);
    bool is_stopping = false;
#if !defined(_WIN32)
    is_stopping = helper.stopping_;
#endif

    if (helper.watchdogs_.empty() && !is_stopping) {
      helper.has_pending_signal_ = true;
    }

    for (auto it = helper.watchdogs_.rbegin(); it != helper.watchdogs_.rend(); ++it) {
      if ((*it)->HandleSigint() == SignalPropagation::kStopPropagation) {
        break;
      }
    }

    return is_stopping;
  }

#if !defined(_WIN32)
  static void* Run(void* arg) {
    SigintWatchdogHelper* helper = static_cast<SigintWatchdogHelper*>(arg);
    bool stopping = false;
    do {
      if (!WaitSignalSemaphore(&helper->sem_)) {
        break;
      }
      stopping = InformWatchdogsAboutSignal();
    } while (!stopping);
    return nullptr;
  }

  static void HandleSignal(int /*signal*/, siginfo_t* /*info*/, void* /*ucontext*/) {
    SigintWatchdogHelper& helper = GetInstance();
    if (helper.sem_initialized_) {
      (void)PostSignalSemaphore(&helper.sem_);
    }
  }
#endif

  std::mutex mutex_;
  std::mutex list_mutex_;
  std::vector<SigintWatchdogBase*> watchdogs_;
  int start_stop_count_ = 0;
  bool has_pending_signal_ = false;

#if !defined(_WIN32)
  bool sem_initialized_ = false;
  bool has_running_thread_ = false;
  bool stopping_ = false;
  bool has_previous_sigint_action_ = false;
  pthread_t thread_{};
  SignalSemaphore sem_{};
  struct sigaction previous_sigint_action_{};
#endif
};

class SigintWatchdog : public SigintWatchdogBase {
 public:
  explicit SigintWatchdog(v8::Isolate* isolate, bool* received_signal)
      : isolate_(isolate), received_signal_(received_signal) {
    std::lock_guard<std::mutex> action_lock(SigintWatchdogHelper::GetActionMutex());
    SigintWatchdogHelper::GetInstance().Register(this);
    (void)SigintWatchdogHelper::GetInstance().Start();
  }

  ~SigintWatchdog() override {
    std::lock_guard<std::mutex> action_lock(SigintWatchdogHelper::GetActionMutex());
    SigintWatchdogHelper::GetInstance().Unregister(this);
    (void)SigintWatchdogHelper::GetInstance().Stop();
  }

  SignalPropagation HandleSigint() override {
    if (received_signal_ != nullptr) {
      *received_signal_ = true;
    }
    isolate_->TerminateExecution();
    return SignalPropagation::kStopPropagation;
  }

 private:
  v8::Isolate* isolate_ = nullptr;
  bool* received_signal_ = nullptr;
};

std::string V8ValueToUtf8(v8::Isolate* isolate, v8::Local<v8::Value> value) {
  if (value.IsEmpty()) return {};
  v8::String::Utf8Value utf8(isolate, value);
  if (*utf8 == nullptr) return {};
  return std::string(*utf8, utf8.length());
}

std::string SerializeModuleRequestKey(const std::string& specifier,
                                      const std::vector<ModuleImportAttributeRecord>& attributes) {
  std::string key = specifier;
  for (const auto& attr : attributes) {
    key.push_back('\0');
    key.append(attr.key);
    key.push_back('\0');
    key.append(attr.value);
  }
  return key;
}

napi_env GetModuleWrapEnvForIsolate(v8::Isolate* isolate) {
  std::lock_guard<std::mutex> lock(g_module_wrap_mu);
  for (auto& entry : g_module_wrap_states) {
    if (entry.first != nullptr && entry.first->isolate == isolate) return entry.first;
  }
  return nullptr;
}

ModuleWrapBindingState* GetModuleWrapState(napi_env env) {
  if (env == nullptr) return nullptr;
  return &g_module_wrap_states[env];
}

void RemoveModuleRecord(napi_env env, ModuleWrapRecord* record) {
  if (env == nullptr || record == nullptr) return;
  auto* state = GetModuleWrapState(env);
  if (state == nullptr) return;
  auto& modules = state->modules;
  modules.erase(std::remove(modules.begin(), modules.end(), record), modules.end());
}

void DestroyModuleRecord(ModuleWrapRecord* record) {
  if (record == nullptr || record->env == nullptr) return;
  napi_env env = record->env;
  RemoveModuleRecord(env, record);
  ResetRef(env, &record->wrapper_ref);
  ResetRef(env, &record->synthetic_eval_steps_ref);
  ResetRef(env, &record->source_object_ref);
  ResetRef(env, &record->host_defined_option_ref);
  record->context.Reset();
  record->module.Reset();
  env->release(record);
}

bool TryGetInternalBindingSymbol(napi_env env,
                                 const char* binding_name,
                                 const char* symbol_name,
                                 v8::Local<v8::Symbol>* out) {
  if (out == nullptr) return false;
  *out = v8::Local<v8::Symbol>();

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Object> global = context->Global();

  v8::Local<v8::Value> internal_binding_value;
  if ((!global->Get(context, OneByteString(isolate, "internalBinding")).ToLocal(&internal_binding_value) ||
       !internal_binding_value->IsFunction()) &&
      (!global->Get(context, OneByteString(isolate, "getInternalBinding")).ToLocal(&internal_binding_value) ||
       !internal_binding_value->IsFunction())) {
    return false;
  }

  v8::Local<v8::Function> internal_binding = internal_binding_value.As<v8::Function>();
  v8::Local<v8::Value> argv[1] = {OneByteString(isolate, binding_name)};
  v8::Local<v8::Value> binding_value;
  if (!internal_binding->Call(context, global, 1, argv).ToLocal(&binding_value) || !binding_value->IsObject()) {
    return false;
  }

  v8::Local<v8::Object> binding = binding_value.As<v8::Object>();
  v8::Local<v8::Value> symbols_value;
  if (!binding->Get(context, OneByteString(isolate, "privateSymbols")).ToLocal(&symbols_value) ||
      !symbols_value->IsObject()) {
    return false;
  }

  v8::Local<v8::Object> symbols = symbols_value.As<v8::Object>();
  v8::Local<v8::Value> symbol_value;
  if (!symbols->Get(context, OneByteString(isolate, symbol_name)).ToLocal(&symbol_value) || !symbol_value->IsSymbol()) {
    return false;
  }

  *out = symbol_value.As<v8::Symbol>();
  return true;
}

v8::Local<v8::PrimitiveArray> HostDefinedOptions(v8::Isolate* isolate, v8::Local<v8::Symbol> id_symbol) {
  v8::Local<v8::PrimitiveArray> out = v8::PrimitiveArray::New(isolate, kHostDefinedOptionsLength);
  out->Set(isolate, kHostDefinedOptionsId, id_symbol.IsEmpty() ? v8::Undefined(isolate) : id_symbol);
  return out;
}

bool ReadArrayBufferViewBytes(v8::Local<v8::Value> value,
                              const uint8_t** data_out,
                              size_t* size_out) {
  if (data_out == nullptr || size_out == nullptr || value.IsEmpty() || !value->IsArrayBufferView()) {
    return false;
  }
  v8::Local<v8::ArrayBufferView> view = value.As<v8::ArrayBufferView>();
  std::shared_ptr<v8::BackingStore> store = view->Buffer()->GetBackingStore();
  if (!store || store->Data() == nullptr) {
    *data_out = nullptr;
    *size_out = 0;
    return true;
  }
  *data_out = static_cast<const uint8_t*>(store->Data()) + view->ByteOffset();
  *size_out = view->ByteLength();
  return true;
}

bool CreateNodeBufferFromBytes(napi_env env, const uint8_t* data, size_t size, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();

  std::unique_ptr<v8::BackingStore> store = v8::ArrayBuffer::NewBackingStore(isolate, size);
  if (!store) return false;
  if (size > 0 && data != nullptr) {
    std::memcpy(store->Data(), data, size);
  }

  v8::Local<v8::ArrayBuffer> ab = v8::ArrayBuffer::New(isolate, std::move(store));
  v8::Local<v8::Uint8Array> view = v8::Uint8Array::New(ab, 0, size);

  v8::Local<v8::Object> global = context->Global();
  v8::Local<v8::Value> buffer_ctor_value;
  if (global->Get(context, OneByteString(isolate, "Buffer")).ToLocal(&buffer_ctor_value) &&
      buffer_ctor_value->IsFunction()) {
    v8::Local<v8::Object> buffer_ctor_obj = buffer_ctor_value.As<v8::Object>();
    v8::Local<v8::Value> from_value;
    if (buffer_ctor_obj->Get(context, OneByteString(isolate, "from")).ToLocal(&from_value) &&
        from_value->IsFunction()) {
      v8::Local<v8::Value> argv[1] = {view};
      v8::Local<v8::Value> buffer_out;
      if (from_value.As<v8::Function>()->Call(context, buffer_ctor_value, 1, argv).ToLocal(&buffer_out)) {
        *out = napi_v8_wrap_value(env, buffer_out);
        return *out != nullptr;
      }
    }
  }

  *out = napi_v8_wrap_value(env, view);
  return *out != nullptr;
}

bool SnapshotOwnProperties(v8::Isolate* isolate,
                           v8::Local<v8::Context> context,
                           v8::Local<v8::Object> object,
                           std::vector<SavedOwnProperty>* out) {
  if (out == nullptr) return false;
  out->clear();

  v8::Local<v8::Array> names;
  if (!object
           ->GetPropertyNames(context,
                              v8::KeyCollectionMode::kOwnOnly,
                              static_cast<v8::PropertyFilter>(v8::PropertyFilter::ALL_PROPERTIES),
                              v8::IndexFilter::kIncludeIndices,
                              v8::KeyConversionMode::kKeepNumbers)
           .ToLocal(&names)) {
    return false;
  }

  out->reserve(names->Length());
  for (uint32_t i = 0; i < names->Length(); ++i) {
    v8::Local<v8::Value> key_value;
    if (!names->Get(context, i).ToLocal(&key_value) || !key_value->IsName()) {
      continue;
    }
    v8::Local<v8::Name> key = key_value.As<v8::Name>();
    v8::Local<v8::Value> value;
    if (!object->Get(context, key).ToLocal(&value)) {
      return false;
    }
    SavedOwnProperty saved;
    saved.key.Reset(isolate, key);
    saved.value.Reset(isolate, value);
    out->push_back(std::move(saved));
  }
  return true;
}

bool RestoreOwnProperties(v8::Isolate* isolate,
                          v8::Local<v8::Context> context,
                          v8::Local<v8::Object> object,
                          const std::vector<SavedOwnProperty>& saved_properties) {
  for (const SavedOwnProperty& saved : saved_properties) {
    v8::Local<v8::Name> key = saved.key.Get(isolate);
    v8::Local<v8::Value> value = saved.value.Get(isolate);
    if (key.IsEmpty() || !object->Set(context, key, value).FromMaybe(false)) {
      return false;
    }
  }
  return true;
}

void CleanupContextRecords(void* arg) {
  napi_env env = static_cast<napi_env>(arg);

  std::lock_guard<std::mutex> lock(g_context_mu);
  g_context_cleanup_hooks.erase(env);
  auto it = g_context_records.find(env);
  if (it == g_context_records.end()) return;
  for (auto& rec : it->second) {
    if (rec.key_ref != nullptr && env->context_token_unassign_callback != nullptr) {
      env->context_token_unassign_callback(
          env, rec.key_ref, env->context_token_callback_data);
    }
    if (rec.key_ref != nullptr) {
      napi_delete_reference(env, rec.key_ref);
      rec.key_ref = nullptr;
    }
    rec.context.Reset();
    rec.own_microtask_queue.reset();
  }
  g_context_records.erase(it);
}

void EnsureContextCleanupHook(napi_env env) {
  auto [it, inserted] = g_context_cleanup_hooks.emplace(env);
  if (!inserted) return;
  if (napi_add_env_cleanup_hook(env, CleanupContextRecords, env) != napi_ok) {
    g_context_cleanup_hooks.erase(it);
  }
}

void CleanupModuleWrapState(void* arg) {
  napi_env env = static_cast<napi_env>(arg);

  std::lock_guard<std::mutex> lock(g_module_wrap_mu);
  g_module_wrap_cleanup_hooks.erase(env);
  auto it = g_module_wrap_states.find(env);
  if (it == g_module_wrap_states.end()) return;

  for (ModuleWrapRecord* record : it->second.modules) {
    if (record == nullptr) continue;
    ResetRef(env, &record->wrapper_ref);
    ResetRef(env, &record->synthetic_eval_steps_ref);
    ResetRef(env, &record->source_object_ref);
    ResetRef(env, &record->host_defined_option_ref);
    record->context.Reset();
    record->module.Reset();
    env->release(record);
  }

  ResetRef(env, &it->second.import_module_dynamically_ref);
  ResetRef(env, &it->second.initialize_import_meta_ref);
  g_module_wrap_states.erase(it);
}

void EnsureModuleWrapCleanupHook(napi_env env) {
  auto [it, inserted] = g_module_wrap_cleanup_hooks.emplace(env);
  if (!inserted) return;
  if (napi_add_env_cleanup_hook(env, CleanupModuleWrapState, env) != napi_ok) {
    g_module_wrap_cleanup_hooks.erase(it);
  }
}

ContextRecord* FindRecordByKey(napi_env env, napi_value key) {
  auto it = g_context_records.find(env);
  if (it == g_context_records.end()) return nullptr;
  for (auto& rec : it->second) {
    if (rec.key_ref == nullptr) continue;
    napi_value candidate = nullptr;
    if (napi_get_reference_value(env, rec.key_ref, &candidate) != napi_ok || candidate == nullptr) continue;
    bool same = false;
    if (napi_strict_equals(env, candidate, key, &same) == napi_ok && same) {
      return &rec;
    }
  }
  return nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok) return nullptr;
  return out;
}

bool CreateCodeError(napi_env env, const char* code, const std::string& message, napi_value* error_out) {
  if (error_out == nullptr) return false;
  *error_out = nullptr;
  napi_value message_value = nullptr;
  napi_value error = nullptr;
  if (napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      napi_create_error(env, nullptr, message_value, &error) != napi_ok ||
      error == nullptr) {
    return false;
  }
  if (code != nullptr) {
    napi_value code_value = nullptr;
    napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value);
    napi_set_named_property(env, error, "code", code_value);
  }
  *error_out = error;
  return true;
}

void ThrowCodeError(napi_env env, const char* code, const std::string& message) {
  napi_value error = nullptr;
  if (CreateCodeError(env, code, message, &error) && error != nullptr) {
    napi_throw(env, error);
  }
}

std::string RequireAsyncModuleMessage(const std::string& filename,
                                      const std::string& parent_filename) {
  std::string message =
      "require() cannot be used on an ESM graph with top-level await. "
      "Use import() instead. To see where the top-level await comes from, "
      "use --experimental-print-required-tla.";
  if (!parent_filename.empty()) {
    message += "\n  From " + parent_filename + " ";
  }
  if (!filename.empty()) {
    message += "\n  Requiring " + filename + " ";
  }
  return message;
}

void ThrowV8CodeError(v8::Local<v8::Context> context, const char* code, const std::string& message) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) {
    context->GetIsolate()->ThrowException(v8::Exception::Error(
        v8::String::NewFromUtf8(context->GetIsolate(), message.c_str(), v8::NewStringType::kNormal).ToLocalChecked()));
    return;
  }
  napi_value error = nullptr;
  if (!CreateCodeError(env, code, message, &error) || error == nullptr) return;
  context->GetIsolate()->ThrowException(napi_v8_unwrap_value(error));
}

ModuleWrapRecord* FindModuleRecordForModule(napi_env env, v8::Local<v8::Module> module) {
  auto* state = GetModuleWrapState(env);
  if (state == nullptr || module.IsEmpty()) return nullptr;
  for (ModuleWrapRecord* record : state->modules) {
    if (record == nullptr || record->module.IsEmpty()) continue;
    if (record->module.Get(env->isolate) == module) return record;
  }
  return nullptr;
}

ModuleWrapRecord* FindModuleRecordForWrapper(napi_env env, napi_value wrapper) {
  if (env == nullptr || wrapper == nullptr) return nullptr;
  auto* state = GetModuleWrapState(env);
  if (state == nullptr) return nullptr;
  for (ModuleWrapRecord* record : state->modules) {
    if (record == nullptr || record->wrapper_ref == nullptr) continue;
    napi_value candidate = GetRefValue(env, record->wrapper_ref);
    if (candidate == nullptr) continue;
    bool same = false;
    if (napi_strict_equals(env, candidate, wrapper, &same) == napi_ok && same) {
      return record;
    }
  }
  return nullptr;
}

bool SetHostDefinedOptionSymbolOnWrapper(napi_env env, napi_value wrapper, napi_value id_value) {
  if (env == nullptr || wrapper == nullptr) return false;
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Object> wrapper_obj = napi_v8_unwrap_value(wrapper).As<v8::Object>();
  v8::Local<v8::Value> id_raw =
      id_value != nullptr ? napi_v8_unwrap_value(id_value) : v8::Undefined(env->isolate).As<v8::Value>();
  return SetApiPrivate(context, wrapper_obj, "node:host_defined_option_symbol", id_raw);
}

v8::Local<v8::Object> CreateFrozenNullProtoObject(
    napi_env env,
    const std::vector<v8::Local<v8::Name>>& names,
    const std::vector<v8::Local<v8::Value>>& values) {
  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();
  std::vector<v8::Local<v8::Name>> mutable_names(names.begin(), names.end());
  std::vector<v8::Local<v8::Value>> mutable_values(values.begin(), values.end());
  v8::Local<v8::Object> object = v8::Object::New(isolate,
                                                 v8::Null(isolate),
                                                 mutable_names.empty() ? nullptr : mutable_names.data(),
                                                 mutable_values.empty() ? nullptr : mutable_values.data(),
                                                 mutable_names.size());
  object->SetIntegrityLevel(context, v8::IntegrityLevel::kFrozen).Check();
  return object;
}

napi_value GetVmDynamicImportDefaultInternalSymbol(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value out = GetSymbolsBindingProperty(env, "vm_dynamic_import_default_internal");
  return out;
}

napi_value GetSymbolsBindingProperty(napi_env env, const char* property_name) {
  if (env == nullptr || property_name == nullptr) return nullptr;
  napi_value global = nullptr;
  napi_get_global(env, &global);
  if (global == nullptr) return nullptr;

  napi_value internal_binding = nullptr;
  napi_valuetype type = napi_undefined;
  if ((napi_get_named_property(env, global, "internalBinding", &internal_binding) != napi_ok ||
       internal_binding == nullptr ||
       napi_typeof(env, internal_binding, &type) != napi_ok ||
       type != napi_function) &&
      (napi_get_named_property(env, global, "getInternalBinding", &internal_binding) != napi_ok ||
       internal_binding == nullptr ||
       napi_typeof(env, internal_binding, &type) != napi_ok ||
       type != napi_function)) {
    return nullptr;
  }

  napi_value symbols_name = nullptr;
  napi_create_string_utf8(env, "symbols", NAPI_AUTO_LENGTH, &symbols_name);
  napi_value symbols_binding = nullptr;
  if (napi_call_function(env, global, internal_binding, 1, &symbols_name, &symbols_binding) != napi_ok ||
      symbols_binding == nullptr) {
    return nullptr;
  }

  napi_value out = nullptr;
  if (napi_get_named_property(env, symbols_binding, property_name, &out) != napi_ok) {
    return nullptr;
  }
  return out;
}

napi_value GetSourceTextModuleDefaultHdoSymbol(napi_env env) {
  return GetSymbolsBindingProperty(env, "source_text_module_default_hdo");
}

bool PopulateModuleRequests(napi_env env,
                            ModuleWrapRecord* record,
                            v8::Local<v8::Context> context,
                            v8::Local<v8::Module> module) {
  if (env == nullptr || record == nullptr || module.IsEmpty()) return false;
  record->module_requests.clear();
  record->resolve_cache.clear();

  v8::Local<v8::FixedArray> raw_requests = module->GetModuleRequests();
  record->module_requests.reserve(raw_requests->Length());
  for (int i = 0; i < raw_requests->Length(); ++i) {
    v8::Local<v8::Value> request_value = raw_requests->Get(context, i).As<v8::Value>();
    if (request_value.IsEmpty() || !request_value->IsModuleRequest()) {
      return false;
    }
    v8::Local<v8::ModuleRequest> request = request_value.As<v8::ModuleRequest>();

    ModuleRequestRecord out;
    out.specifier = V8ValueToUtf8(env->isolate, request->GetSpecifier());

    v8::Local<v8::FixedArray> raw_attributes = request->GetImportAttributes();
    for (int j = 0; j < raw_attributes->Length(); j += 3) {
      v8::Local<v8::Value> key_value = raw_attributes->Get(context, j).As<v8::Value>();
      v8::Local<v8::Value> value_value = raw_attributes->Get(context, j + 1).As<v8::Value>();
      if (key_value.IsEmpty() || value_value.IsEmpty()) {
        return false;
      }
      out.attributes.push_back({V8ValueToUtf8(env->isolate, key_value), V8ValueToUtf8(env->isolate, value_value)});
    }
    out.phase = request->GetPhase() == v8::ModuleImportPhase::kSource ? 1 : 2;

    const std::string key = SerializeModuleRequestKey(out.specifier, out.attributes);
    if (record->resolve_cache.find(key) == record->resolve_cache.end()) {
      record->resolve_cache.emplace(key, static_cast<uint32_t>(i));
    }
    record->module_requests.push_back(std::move(out));
  }
  return true;
}

v8::MaybeLocal<v8::Module> ModuleResolveCallback(v8::Local<v8::Context> context,
                                                 v8::Local<v8::String> specifier,
                                                 v8::Local<v8::FixedArray> import_attributes,
                                                 v8::Local<v8::Module> referrer) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return v8::MaybeLocal<v8::Module>();
  ModuleWrapRecord* dependent = FindModuleRecordForModule(env, referrer);
  if (dependent == nullptr || dependent->linked_requests.empty()) {
    ThrowV8CodeError(context, "ERR_VM_MODULE_LINK_FAILURE", "Module is not linked");
    return v8::MaybeLocal<v8::Module>();
  }

  std::vector<ModuleImportAttributeRecord> attributes;
  for (int i = 0; i < import_attributes->Length(); i += 3) {
    v8::Local<v8::Value> key_value = import_attributes->Get(context, i).As<v8::Value>();
    v8::Local<v8::Value> value_value = import_attributes->Get(context, i + 1).As<v8::Value>();
    if (key_value.IsEmpty() || value_value.IsEmpty()) {
      return v8::MaybeLocal<v8::Module>();
    }
    attributes.push_back({V8ValueToUtf8(env->isolate, key_value), V8ValueToUtf8(env->isolate, value_value)});
  }

  const std::string key = SerializeModuleRequestKey(V8ValueToUtf8(env->isolate, specifier), attributes);
  auto it = dependent->resolve_cache.find(key);
  if (it == dependent->resolve_cache.end() || it->second >= dependent->linked_requests.size() ||
      dependent->linked_requests[it->second] == nullptr || dependent->linked_requests[it->second]->module.IsEmpty()) {
    ThrowV8CodeError(context, "ERR_VM_MODULE_LINK_FAILURE", "Module request is not cached");
    return v8::MaybeLocal<v8::Module>();
  }
  return dependent->linked_requests[it->second]->module.Get(env->isolate);
}

v8::MaybeLocal<v8::Object> ModuleResolveSourceCallback(v8::Local<v8::Context> context,
                                                       v8::Local<v8::String> specifier,
                                                       v8::Local<v8::FixedArray> import_attributes,
                                                       v8::Local<v8::Module> referrer) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return v8::MaybeLocal<v8::Object>();
  ModuleWrapRecord* dependent = FindModuleRecordForModule(env, referrer);
  if (dependent == nullptr || dependent->linked_requests.empty()) {
    ThrowV8CodeError(context, "ERR_VM_MODULE_LINK_FAILURE", "Module is not linked");
    return v8::MaybeLocal<v8::Object>();
  }

  std::vector<ModuleImportAttributeRecord> attributes;
  for (int i = 0; i < import_attributes->Length(); i += 3) {
    v8::Local<v8::Value> key_value = import_attributes->Get(context, i).As<v8::Value>();
    v8::Local<v8::Value> value_value = import_attributes->Get(context, i + 1).As<v8::Value>();
    if (key_value.IsEmpty() || value_value.IsEmpty()) {
      return v8::MaybeLocal<v8::Object>();
    }
    attributes.push_back({V8ValueToUtf8(env->isolate, key_value), V8ValueToUtf8(env->isolate, value_value)});
  }

  const std::string key = SerializeModuleRequestKey(V8ValueToUtf8(env->isolate, specifier), attributes);
  auto it = dependent->resolve_cache.find(key);
  if (it == dependent->resolve_cache.end() || it->second >= dependent->linked_requests.size() ||
      dependent->linked_requests[it->second] == nullptr) {
    ThrowV8CodeError(context, "ERR_VM_MODULE_LINK_FAILURE", "Module request is not cached");
    return v8::MaybeLocal<v8::Object>();
  }

  napi_value source_object = GetRefValue(env, dependent->linked_requests[it->second]->source_object_ref);
  if (source_object == nullptr) {
    ThrowV8CodeError(
        context,
        "ERR_SOURCE_PHASE_NOT_DEFINED",
        "Source phase import object is not defined for module");
    return v8::MaybeLocal<v8::Object>();
  }
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(source_object);
  if (raw.IsEmpty() || !raw->IsObject()) {
    ThrowV8CodeError(
        context,
        "ERR_SOURCE_PHASE_NOT_DEFINED",
        "Source phase import object is not defined for module");
    return v8::MaybeLocal<v8::Object>();
  }
  return raw.As<v8::Object>();
}

v8::MaybeLocal<v8::Value> SyntheticModuleEvaluationSteps(v8::Local<v8::Context> context,
                                                         v8::Local<v8::Module> module) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return v8::MaybeLocal<v8::Value>();
  ModuleWrapRecord* record = FindModuleRecordForModule(env, module);
  if (record == nullptr) return v8::MaybeLocal<v8::Value>();

  napi_value wrapper = GetRefValue(env, record->wrapper_ref);
  napi_value callback = GetRefValue(env, record->synthetic_eval_steps_ref);
  if (wrapper == nullptr || callback == nullptr) return v8::MaybeLocal<v8::Value>();

  napi_value ignored = nullptr;
  if (napi_call_function(env, wrapper, callback, 0, nullptr, &ignored) != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value error = nullptr;
      if (napi_get_and_clear_last_exception(env, &error) == napi_ok && error != nullptr) {
        context->GetIsolate()->ThrowException(napi_v8_unwrap_value(error));
      }
    }
    return v8::MaybeLocal<v8::Value>();
  }

  v8::Local<v8::Promise::Resolver> resolver;
  if (!v8::Promise::Resolver::New(context).ToLocal(&resolver)) {
    return v8::MaybeLocal<v8::Value>();
  }
  if (resolver->Resolve(context, v8::Undefined(context->GetIsolate())).IsNothing()) {
    return v8::MaybeLocal<v8::Value>();
  }
  return resolver->GetPromise();
}

v8::Local<v8::Object> CreateDynamicImportAttributesObject(
    napi_env env,
    v8::Local<v8::FixedArray> import_attributes) {
  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();
  std::vector<v8::Local<v8::Name>> names;
  std::vector<v8::Local<v8::Value>> values;
  names.reserve(import_attributes->Length() / 2);
  values.reserve(import_attributes->Length() / 2);
  for (int i = 0; i < import_attributes->Length(); i += 2) {
    v8::Local<v8::Value> key = import_attributes->Get(context, i).As<v8::Value>();
    v8::Local<v8::Value> value = import_attributes->Get(context, i + 1).As<v8::Value>();
    if (key.IsEmpty() || value.IsEmpty() || !key->IsName()) continue;
    names.push_back(key.As<v8::Name>());
    values.push_back(value);
  }
  return CreateFrozenNullProtoObject(env, names, values);
}

v8::MaybeLocal<v8::Promise> ImportModuleDynamicallyWithPhase(
    v8::Local<v8::Context> context,
    v8::Local<v8::Data> host_defined_options,
    v8::Local<v8::Value> resource_name,
    v8::Local<v8::String> specifier,
    v8::ModuleImportPhase phase,
    v8::Local<v8::FixedArray> import_attributes) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return v8::MaybeLocal<v8::Promise>();

  auto* state = GetModuleWrapState(env);
  if (state == nullptr) return v8::MaybeLocal<v8::Promise>();
  napi_value callback = GetRefValue(env, state->import_module_dynamically_ref);
  if (callback == nullptr) return v8::MaybeLocal<v8::Promise>();

  v8::Isolate* isolate = context->GetIsolate();
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Context::Scope context_scope(context);

  v8::Local<v8::Value> id = v8::Undefined(isolate);
  if (!host_defined_options.IsEmpty() && host_defined_options->IsFixedArray()) {
    v8::Local<v8::FixedArray> options = host_defined_options.As<v8::FixedArray>();
    if (options->Length() == kHostDefinedOptionsLength) {
      id = options->Get(context, kHostDefinedOptionsId).As<v8::Value>();
    }
  }
  if (id->IsUndefined()) {
    v8::Local<v8::Value> global_id;
    if (context->Global()
            ->GetPrivate(context, ApiPrivate(isolate, "node:host_defined_option_symbol"))
            .ToLocal(&global_id)) {
      id = global_id;
    }
  }

  napi_value phase_value = nullptr;
  napi_create_int32(env, phase == v8::ModuleImportPhase::kSource ? 1 : 2, &phase_value);
  napi_value argv[5] = {
      napi_v8_wrap_value(env, id),
      napi_v8_wrap_value(env, specifier),
      phase_value,
      napi_v8_wrap_value(env, CreateDynamicImportAttributesObject(env, import_attributes)),
      napi_v8_wrap_value(env, resource_name),
  };

  napi_value global = nullptr;
  napi_get_global(env, &global);
  napi_value result = nullptr;
  if (napi_call_function(env, global, callback, 5, argv, &result) != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value error = nullptr;
      if (napi_get_and_clear_last_exception(env, &error) == napi_ok && error != nullptr) {
        isolate->ThrowException(napi_v8_unwrap_value(error));
      }
    }
    return v8::MaybeLocal<v8::Promise>();
  }

  v8::Local<v8::Promise::Resolver> resolver;
  if (!v8::Promise::Resolver::New(context).ToLocal(&resolver)) {
    return v8::MaybeLocal<v8::Promise>();
  }
  if (resolver->Resolve(context, napi_v8_unwrap_value(result)).IsNothing()) {
    return v8::MaybeLocal<v8::Promise>();
  }
  return handle_scope.Escape(resolver->GetPromise());
}

v8::MaybeLocal<v8::Promise> ImportModuleDynamically(
    v8::Local<v8::Context> context,
    v8::Local<v8::Data> host_defined_options,
    v8::Local<v8::Value> resource_name,
    v8::Local<v8::String> specifier,
    v8::Local<v8::FixedArray> import_attributes) {
  return ImportModuleDynamicallyWithPhase(
      context, host_defined_options, resource_name, specifier, v8::ModuleImportPhase::kEvaluation, import_attributes);
}

void HostInitializeImportMetaObject(v8::Local<v8::Context> context,
                                    v8::Local<v8::Module> module,
                                    v8::Local<v8::Object> meta) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return;
  auto it = g_module_wrap_states.find(env);
  if (it == g_module_wrap_states.end()) return;
  ModuleWrapRecord* record = FindModuleRecordForModule(env, module);
  if (record == nullptr) return;

  napi_value callback = GetRefValue(env, it->second.initialize_import_meta_ref);
  napi_value wrapper = GetRefValue(env, record->wrapper_ref);
  napi_value id_value = GetRefValue(env, record->host_defined_option_ref);
  if (callback == nullptr || wrapper == nullptr || id_value == nullptr) return;

  napi_value meta_value = napi_v8_wrap_value(env, meta);
  napi_value argv[3] = {id_value, meta_value, wrapper};
  napi_value ignored = nullptr;
  napi_value global = nullptr;
  napi_get_global(env, &global);
  (void)napi_call_function(env, global, callback, 3, argv, &ignored);
}

v8::MaybeLocal<v8::Module> LinkRequiredFacadeOriginal(v8::Local<v8::Context> context,
                                                      v8::Local<v8::String> specifier,
                                                      v8::Local<v8::FixedArray> /*import_attributes*/,
                                                      v8::Local<v8::Module> /*referrer*/) {
  napi_env env = GetModuleWrapEnvForIsolate(context->GetIsolate());
  if (env == nullptr) return v8::MaybeLocal<v8::Module>();
  auto* state = GetModuleWrapState(env);
  if (state == nullptr || state->temporary_required_module_facade_original == nullptr ||
      state->temporary_required_module_facade_original->module.IsEmpty()) {
    return v8::MaybeLocal<v8::Module>();
  }
  if (V8ValueToUtf8(context->GetIsolate(), specifier) != "original") {
    return v8::MaybeLocal<v8::Module>();
  }
  return state->temporary_required_module_facade_original->module.Get(context->GetIsolate());
}

bool StoreRecord(napi_env env,
                 napi_value key,
                 v8::Local<v8::Context> context,
                 std::unique_ptr<v8::MicrotaskQueue> own_microtask_queue) {
  EnsureContextCleanupHook(env);

  ContextRecord record;
  if (napi_create_reference(env, key, 1, &record.key_ref) != napi_ok || record.key_ref == nullptr) {
    return false;
  }
  record.context.Reset(env->isolate, context);
  record.own_microtask_queue = std::move(own_microtask_queue);
  if (env->context_token_assign_callback != nullptr) {
    env->context_token_assign_callback(env, record.key_ref, env->context_token_callback_data);
  }
  g_context_records[env].push_back(std::move(record));
  return true;
}

bool ResolveContextFromKey(napi_env env,
                           napi_value key,
                           v8::Local<v8::Context>* context_out,
                           v8::MicrotaskQueue** microtask_queue_out) {
  if (context_out == nullptr || microtask_queue_out == nullptr) return false;
  *microtask_queue_out = nullptr;
  ContextRecord* rec = FindRecordByKey(env, key);
  if (rec == nullptr) return false;
  v8::Local<v8::Context> context = rec->context.Get(env->isolate);
  if (context.IsEmpty()) return false;
  *context_out = context;
  *microtask_queue_out = rec->own_microtask_queue ? rec->own_microtask_queue.get() : nullptr;
  return true;
}

bool CompileAsModule(v8::Isolate* isolate,
                     v8::Local<v8::Context> context,
                     napi_env env,
                     v8::Local<v8::String> code,
                     v8::Local<v8::String> resource_name) {
  napi_value hdo_value = GetSourceTextModuleDefaultHdoSymbol(env);
  v8::Local<v8::Symbol> hdo_symbol;
  if (hdo_value != nullptr) {
    v8::Local<v8::Value> raw = napi_v8_unwrap_value(hdo_value);
    if (!raw.IsEmpty() && raw->IsSymbol()) {
      hdo_symbol = raw.As<v8::Symbol>();
    }
  }

  v8::TryCatch tc(isolate);
  v8::ScriptOrigin origin(resource_name,
                          0,
                          0,
                          true,
                          -1,
                          v8::Local<v8::Value>(),
                          false,
                          false,
                          true,
                          HostDefinedOptions(isolate, hdo_symbol));
  v8::ScriptCompiler::Source source(code, origin);
  v8::Local<v8::Module> module;
  if (v8::ScriptCompiler::CompileModule(isolate, &source).ToLocal(&module)) {
    return true;
  }
  return false;
}

bool ShouldRetryAsEsm(v8::Isolate* isolate,
                      v8::Local<v8::Context> context,
                      napi_env env,
                      v8::Local<v8::Value> message,
                      v8::Local<v8::String> code,
                      v8::Local<v8::String> resource_name) {
  const std::string message_text = V8ValueToUtf8(isolate, message);

  for (const auto& error_message : kEsmSyntaxErrorMessages) {
    if (message_text.find(error_message) != std::string::npos) {
      return true;
    }
  }

  bool maybe_valid_in_esm = false;
  for (const auto& error_message : kThrowsOnlyInCjsErrorMessages) {
    if (message_text.find(error_message) != std::string::npos) {
      maybe_valid_in_esm = true;
      break;
    }
  }
  if (!maybe_valid_in_esm) {
    for (const auto& error_message : kMaybeTopLevelAwaitErrors) {
      if (message_text.find(error_message) != std::string::npos) {
        maybe_valid_in_esm = true;
        break;
      }
    }
  }
  if (!maybe_valid_in_esm) {
    return false;
  }

  return CompileAsModule(isolate, context, env, code, resource_name);
}

v8::MaybeLocal<v8::Function> CompileCjsFunction(v8::Local<v8::Context> context,
                                                v8::Local<v8::String> code,
                                                v8::Local<v8::String> filename,
                                                bool is_cjs_scope,
                                                v8::Local<v8::Symbol> host_id_symbol = v8::Local<v8::Symbol>()) {
  v8::Isolate* isolate = context->GetIsolate();

  v8::ScriptOrigin origin(filename,
                          0,
                          0,
                          true,
                          -1,
                          v8::Local<v8::Value>(),
                          false,
                          false,
                          false,
                          HostDefinedOptions(isolate, host_id_symbol));
  v8::ScriptCompiler::Source source(code, origin);

  std::vector<v8::Local<v8::String>> params;
  if (is_cjs_scope) {
    params.emplace_back(OneByteString(isolate, "exports"));
    params.emplace_back(OneByteString(isolate, "require"));
    params.emplace_back(OneByteString(isolate, "module"));
    params.emplace_back(OneByteString(isolate, "__filename"));
    params.emplace_back(OneByteString(isolate, "__dirname"));
  }

  return v8::ScriptCompiler::CompileFunction(context,
                                             &source,
                                             params.size(),
                                             params.empty() ? nullptr : params.data(),
                                             0,
                                             nullptr,
                                             v8::ScriptCompiler::kNoCompileOptions,
                                             v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason);
}

// ---- bytecode handles (unofficial_napi_js_source) ----

struct BytecodeRecord {
  std::vector<uint8_t> bytes;
  std::string source_utf8;
  std::string filename_utf8;
  int32_t shape = unofficial_napi_bytecode_shape_script;
  std::vector<std::string> params;
  int32_t line_offset = 0;
  int32_t column_offset = 0;
  // Live artifact (one per shape). The function artifact is bound to the
  // context and host-defined-option symbol it was compiled with; consumers
  // re-consume `bytes` when their compile inputs differ. (Script shape keeps
  // no live artifact: run_script always re-consumes `bytes`, and V8's
  // CompilationCache already deduplicates compiling identical source.)
  v8::Global<v8::Function> function;
  v8::Global<v8::Context> function_context;
  v8::Global<v8::Module> module;
  // Host-defined-option symbol baked into the artifact's origin; consumers
  // with a different symbol re-consume `bytes` instead of reusing it.
  v8::Global<v8::Symbol> host_symbol;
};

BytecodeRecord* BytecodeRecordFromSource(const unofficial_napi_js_source* source) {
  return source != nullptr ? static_cast<BytecodeRecord*>(source->bytecode) : nullptr;
}

bool ReadParamsArray(napi_env env, napi_value params_or_undefined, std::vector<std::string>* out) {
  out->clear();
  if (IsNullish(env, params_or_undefined)) return true;
  v8::Local<v8::Value> value = napi_v8_unwrap_value(params_or_undefined);
  if (value.IsEmpty() || !value->IsArray()) return false;
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Array> array = value.As<v8::Array>();
  out->reserve(array->Length());
  for (uint32_t i = 0; i < array->Length(); ++i) {
    v8::Local<v8::Value> item;
    if (!array->Get(context, i).ToLocal(&item) || !item->IsString()) return false;
    out->push_back(V8ValueToUtf8(env->isolate, item));
  }
  return true;
}

std::vector<v8::Local<v8::String>> ToV8Params(v8::Isolate* isolate,
                                              const std::vector<std::string>& params) {
  std::vector<v8::Local<v8::String>> out;
  out.reserve(params.size());
  for (const auto& param : params) {
    out.push_back(v8::String::NewFromUtf8(isolate, param.c_str(), v8::NewStringType::kNormal,
                                          static_cast<int>(param.size()))
                      .ToLocalChecked());
  }
  return out;
}

v8::ScriptOrigin BytecodeOrigin(v8::Isolate* isolate,
                                v8::Local<v8::String> filename,
                                const BytecodeRecord& record,
                                v8::Local<v8::Symbol> host_id_symbol) {
  return v8::ScriptOrigin(filename,
                          record.line_offset,
                          record.column_offset,
                          true,
                          -1,
                          v8::Local<v8::Value>(),
                          false,
                          false,
                          record.shape == unofficial_napi_bytecode_shape_module,
                          HostDefinedOptions(isolate, host_id_symbol));
}

v8::Local<v8::Symbol> DefaultCjsHostSymbol(napi_env env) {
  v8::Local<v8::Symbol> host_id_symbol;
  napi_value host_id_value = GetVmDynamicImportDefaultInternalSymbol(env);
  if (host_id_value != nullptr) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_id_value);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }
  return host_id_symbol;
}

// Compiles record->source_utf8 per record->shape and stores the live artifact.
// consume == true offers record->bytes as a code cache (rejected_out gets
// V8's verdict); consume == false compiles eagerly and fills record->bytes
// from the produced code cache. Returns false on a compile error (exception
// captured in try_catch).
bool CompileBytecodeRecord(napi_env env,
                           BytecodeRecord* record,
                           v8::Local<v8::Symbol> host_id_symbol,
                           bool consume,
                           bool* rejected_out) {
  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();

  if (rejected_out != nullptr) *rejected_out = false;

  v8::Local<v8::String> code_str =
      v8::String::NewFromUtf8(isolate, record->source_utf8.c_str(), v8::NewStringType::kNormal,
                              static_cast<int>(record->source_utf8.size()))
          .ToLocalChecked();
  v8::Local<v8::String> filename_str =
      v8::String::NewFromUtf8(isolate, record->filename_utf8.c_str(), v8::NewStringType::kNormal,
                              static_cast<int>(record->filename_utf8.size()))
          .ToLocalChecked();

  v8::ScriptCompiler::CachedData* cached_data = nullptr;
  if (consume) {
    cached_data = new v8::ScriptCompiler::CachedData(record->bytes.data(),
                                                     static_cast<int>(record->bytes.size()),
                                                     v8::ScriptCompiler::CachedData::BufferNotOwned);
  }
  v8::ScriptOrigin origin = BytecodeOrigin(isolate, filename_str, *record, host_id_symbol);
  v8::ScriptCompiler::Source source_obj(code_str, origin, cached_data);
  const v8::ScriptCompiler::CompileOptions options =
      consume ? v8::ScriptCompiler::kConsumeCodeCache : v8::ScriptCompiler::kEagerCompile;

  std::unique_ptr<v8::ScriptCompiler::CachedData> produced;
  switch (record->shape) {
    case unofficial_napi_bytecode_shape_script: {
      v8::Local<v8::UnboundScript> unbound;
      if (!v8::ScriptCompiler::CompileUnboundScript(
               isolate, &source_obj, options, v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
               .ToLocal(&unbound)) {
        return false;
      }
      if (!consume) produced.reset(v8::ScriptCompiler::CreateCodeCache(unbound));
      break;
    }
    case unofficial_napi_bytecode_shape_cjs_function: {
      std::vector<v8::Local<v8::String>> params = ToV8Params(isolate, record->params);
      v8::Local<v8::Function> fn;
      if (!v8::ScriptCompiler::CompileFunction(context,
                                               &source_obj,
                                               params.size(),
                                               params.empty() ? nullptr : params.data(),
                                               0,
                                               nullptr,
                                               options,
                                               v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
               .ToLocal(&fn)) {
        return false;
      }
      if (!consume) produced.reset(v8::ScriptCompiler::CreateCodeCacheForFunction(fn));
      record->function.Reset(isolate, fn);
      record->function_context.Reset(isolate, context);
      break;
    }
    case unofficial_napi_bytecode_shape_module: {
      v8::Local<v8::Module> module;
      if (!v8::ScriptCompiler::CompileModule(
               isolate, &source_obj, options, v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
               .ToLocal(&module)) {
        return false;
      }
      if (!consume && module->IsSourceTextModule()) {
        produced.reset(v8::ScriptCompiler::CreateCodeCache(module->GetUnboundModuleScript()));
      }
      record->module.Reset(isolate, module);
      break;
    }
    default:
      return false;
  }

  if (!host_id_symbol.IsEmpty()) {
    record->host_symbol.Reset(isolate, host_id_symbol);
  }

  if (consume) {
    if (rejected_out != nullptr && source_obj.GetCachedData() != nullptr) {
      *rejected_out = source_obj.GetCachedData()->rejected;
    }
  } else {
    record->bytes.clear();
    if (produced != nullptr && produced->data != nullptr && produced->length > 0) {
      record->bytes.assign(produced->data, produced->data + produced->length);
    }
  }
  return true;
}

}  // namespace

bool NapiV8IsContextifyContext(napi_env env, v8::Local<v8::Context> context) {
  if (env == nullptr || env->isolate == nullptr || context.IsEmpty()) return false;
  std::lock_guard<std::mutex> lock(g_context_mu);
  auto it = g_context_records.find(env);
  if (it == g_context_records.end()) return false;
  for (auto& rec : it->second) {
    v8::Local<v8::Context> candidate = rec.context.Get(env->isolate);
    if (!candidate.IsEmpty() && candidate == context) {
      return true;
    }
  }
  return false;
}

void NapiV8ApplyPromiseHooksToContextifyContexts(napi_env env) {
  if (env == nullptr || env->isolate == nullptr) return;

  std::lock_guard<std::mutex> lock(g_context_mu);
  auto it = g_context_records.find(env);
  if (it == g_context_records.end()) return;

  for (auto& rec : it->second) {
    v8::Local<v8::Context> context = rec.context.Get(env->isolate);
    if (!context.IsEmpty()) {
      NapiV8ApplyPromiseHooksToContext(env, context);
    }
  }
}

extern "C" {

napi_status NAPI_CDECL unofficial_napi_contextify_make_context(
    napi_env env,
    napi_value sandbox_or_symbol,
    napi_value name,
    napi_value origin_or_undefined,
    bool allow_code_gen_strings,
    bool allow_code_gen_wasm,
    bool own_microtask_queue,
    napi_value host_defined_option_id,
    napi_value* result_out) {
  if (env == nullptr || sandbox_or_symbol == nullptr || name == nullptr || result_out == nullptr) {
    return napi_invalid_arg;
  }
  (void)allow_code_gen_wasm;

  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> current = env->context();
  v8::Context::Scope current_scope(current);

  napi_valuetype sandbox_type = napi_undefined;
  if (napi_typeof(env, sandbox_or_symbol, &sandbox_type) != napi_ok) {
    return napi_invalid_arg;
  }

  const bool vanilla = sandbox_type == napi_symbol;
  if (!vanilla && sandbox_type != napi_object && sandbox_type != napi_function) {
    return napi_invalid_arg;
  }

  v8::Local<v8::Symbol> host_id_symbol;
  if (host_defined_option_id != nullptr) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }

  std::unique_ptr<v8::MicrotaskQueue> own_queue;
  v8::MicrotaskQueue* queue = nullptr;
  if (own_microtask_queue) {
    own_queue = v8::MicrotaskQueue::New(isolate, v8::MicrotasksPolicy::kExplicit);
    queue = own_queue.get();
  }

  std::vector<SavedOwnProperty> saved_properties;
  v8::Local<v8::Object> sandbox_object;
  v8::MaybeLocal<v8::Value> maybe_global_object;
  if (!vanilla) {
    v8::Local<v8::Value> sandbox_value = napi_v8_unwrap_value(sandbox_or_symbol);
    if (sandbox_value.IsEmpty() || !sandbox_value->IsObject()) return napi_invalid_arg;
    sandbox_object = sandbox_value.As<v8::Object>();
    if (!SnapshotOwnProperties(isolate, current, sandbox_object, &saved_properties)) {
      return napi_pending_exception;
    }
    maybe_global_object = sandbox_value;
  }

  v8::Local<v8::Context> context = v8::Context::New(isolate,
                                                    nullptr,
                                                    v8::MaybeLocal<v8::ObjectTemplate>(),
                                                    maybe_global_object,
                                                    v8::DeserializeInternalFieldsCallback(),
                                                    queue);

  if (context.IsEmpty()) {
    return napi_pending_exception;
  }

  context->SetSecurityToken(current->GetSecurityToken());
  context->AllowCodeGenerationFromStrings(allow_code_gen_strings);
  NapiV8ApplyPromiseHooksToContext(env, context);

  v8::Local<v8::Object> key_object;
  if (vanilla) {
    key_object = context->Global();
  } else {
    key_object = sandbox_object;
    if (!RestoreOwnProperties(isolate, context, key_object, saved_properties)) {
      return napi_pending_exception;
    }
  }

  napi_value key_napi = napi_v8_wrap_value(env, handle_scope.Escape(key_object));
  if (key_napi == nullptr) return napi_generic_failure;

  {
    std::lock_guard<std::mutex> lock(g_context_mu);
    if (FindRecordByKey(env, key_napi) != nullptr) {
      return napi_invalid_arg;
    }
    if (!StoreRecord(env, key_napi, context, std::move(own_queue))) {
      return napi_generic_failure;
    }
  }

  // Align with lib internal/vm.js isContext() checks in Edge.
  v8::Local<v8::Context> property_context = vanilla ? context : current;
  SetApiPrivate(property_context, key_object, "node:contextify:context", key_object);
  SetApiPrivate(property_context,
                key_object,
                "node:host_defined_option_symbol",
                host_id_symbol.IsEmpty() ? v8::Undefined(isolate) : host_id_symbol.As<v8::Value>());

  *result_out = key_napi;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_bytecode_compile(
    napi_env env,
    napi_value source_text,
    napi_value filename,
    int32_t shape,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    int32_t line_offset,
    int32_t column_offset,
    void** bytecode_out,
    bool* can_parse_as_module_out) {
  if (env == nullptr || source_text == nullptr || filename == nullptr || bytecode_out == nullptr) {
    return napi_invalid_arg;
  }
  *bytecode_out = nullptr;
  if (can_parse_as_module_out != nullptr) *can_parse_as_module_out = false;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  auto record = std::make_unique<BytecodeRecord>();
  record->source_utf8 = ToUtf8String(env, source_text, "");
  record->filename_utf8 = ToUtf8String(env, filename, "[eval]");
  record->shape = shape;
  record->line_offset = line_offset;
  record->column_offset = column_offset;
  if (!ReadParamsArray(env, params_or_undefined, &record->params)) return napi_invalid_arg;

  v8::Local<v8::Symbol> host_id_symbol;
  if (!IsNullish(env, host_defined_option_id)) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }
  // The CJS-function shape defaults to the loader's dynamic-import host
  // symbol so the compiled artifact is directly usable by the CJS loader.
  if (host_id_symbol.IsEmpty() && shape == unofficial_napi_bytecode_shape_cjs_function) {
    host_id_symbol = DefaultCjsHostSymbol(env);
  }

  v8::TryCatch try_catch(isolate);
  if (!CompileBytecodeRecord(env, record.get(), host_id_symbol, /*consume=*/false, nullptr)) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      if (can_parse_as_module_out != nullptr && shape != unofficial_napi_bytecode_shape_module &&
          !try_catch.Message().IsEmpty()) {
        v8::Local<v8::String> code_str =
            v8::String::NewFromUtf8(isolate, record->source_utf8.c_str(), v8::NewStringType::kNormal,
                                    static_cast<int>(record->source_utf8.size()))
                .ToLocalChecked();
        v8::Local<v8::String> filename_str =
            v8::String::NewFromUtf8(isolate, record->filename_utf8.c_str(), v8::NewStringType::kNormal,
                                    static_cast<int>(record->filename_utf8.size()))
                .ToLocalChecked();
        *can_parse_as_module_out = ShouldRetryAsEsm(
            isolate, context, env, try_catch.Message()->Get(), code_str, filename_str);
      }
      if (!try_catch.Message().IsEmpty()) {
        unofficial_napi_internal::AttachSyntaxArrowMessage(
            isolate, context, try_catch.Exception(), try_catch.Message());
      }
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_generic_failure;
  }

  *bytecode_out = record.release();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_bytecode_deserialize(
    napi_env env,
    const uint8_t* bytes,
    size_t byte_length,
    napi_value source_text,
    napi_value filename,
    int32_t shape,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    void** bytecode_out,
    bool* rejected_out) {
  if (env == nullptr || bytes == nullptr || byte_length == 0 || source_text == nullptr ||
      filename == nullptr || bytecode_out == nullptr) {
    return napi_invalid_arg;
  }
  *bytecode_out = nullptr;
  if (rejected_out != nullptr) *rejected_out = false;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  auto record = std::make_unique<BytecodeRecord>();
  record->bytes.assign(bytes, bytes + byte_length);
  record->source_utf8 = ToUtf8String(env, source_text, "");
  record->filename_utf8 = ToUtf8String(env, filename, "[eval]");
  record->shape = shape;
  if (!ReadParamsArray(env, params_or_undefined, &record->params)) return napi_invalid_arg;

  v8::Local<v8::Symbol> host_id_symbol;
  if (!IsNullish(env, host_defined_option_id)) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }
  if (host_id_symbol.IsEmpty() && shape == unofficial_napi_bytecode_shape_cjs_function) {
    host_id_symbol = DefaultCjsHostSymbol(env);
  }

  bool rejected = false;
  v8::TryCatch try_catch(isolate);
  if (!CompileBytecodeRecord(env, record.get(), host_id_symbol, /*consume=*/true, &rejected)) {
    // The retained source is expected to be valid; a compile failure here
    // means the cache/source pairing is unusable. Report as a rejection.
    if (rejected_out != nullptr) *rejected_out = true;
    return napi_ok;
  }
  if (rejected) {
    if (rejected_out != nullptr) *rejected_out = true;
    return napi_ok;
  }

  *bytecode_out = record.release();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_bytecode_serialize(
    napi_env env,
    void* bytecode,
    napi_value* buffer_out) {
  if (env == nullptr || bytecode == nullptr || buffer_out == nullptr) return napi_invalid_arg;
  auto* record = static_cast<BytecodeRecord*>(bytecode);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  napi_value buffer = nullptr;
  if (!CreateNodeBufferFromBytes(env, record->bytes.data(), record->bytes.size(), &buffer) ||
      buffer == nullptr) {
    return napi_generic_failure;
  }
  v8::Local<v8::Value> buffer_value = napi_v8_unwrap_value(buffer);
  if (buffer_value.IsEmpty()) return napi_generic_failure;
  *buffer_out = napi_v8_wrap_value(env, handle_scope.Escape(buffer_value));
  return *buffer_out != nullptr ? napi_ok : napi_generic_failure;
}

napi_status NAPI_CDECL unofficial_napi_bytecode_release(napi_env env, void* bytecode) {
  if (env == nullptr || bytecode == nullptr) return napi_invalid_arg;
  delete static_cast<BytecodeRecord*>(bytecode);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_run_script(
    napi_env env,
    napi_value sandbox_or_null,
    const unofficial_napi_js_source* source,
    napi_value filename,
    int32_t line_offset,
    int32_t column_offset,
    int64_t timeout,
    bool display_errors,
    bool break_on_sigint,
    bool break_on_first_line,
    napi_value host_defined_option_id,
    napi_value* result_out) {
  if (env == nullptr || source == nullptr ||
      (source->text == nullptr && source->bytecode == nullptr) || filename == nullptr ||
      result_out == nullptr) {
    return napi_invalid_arg;
  }
  (void)break_on_first_line;

  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> current = env->context();
  v8::Context::Scope current_scope(current);

  v8::Local<v8::Context> target_context = current;
  v8::MicrotaskQueue* target_queue = nullptr;

  if (!IsNullish(env, sandbox_or_null)) {
    std::lock_guard<std::mutex> lock(g_context_mu);
    if (!ResolveContextFromKey(env, sandbox_or_null, &target_context, &target_queue)) {
      return napi_invalid_arg;
    }
  }

  BytecodeRecord* bytecode_record = BytecodeRecordFromSource(source);
  if (bytecode_record != nullptr &&
      bytecode_record->shape != unofficial_napi_bytecode_shape_script) {
    return napi_invalid_arg;
  }

  v8::Local<v8::String> code =
      bytecode_record != nullptr
          ? v8::String::NewFromUtf8(isolate, bytecode_record->source_utf8.c_str(),
                                    v8::NewStringType::kNormal,
                                    static_cast<int>(bytecode_record->source_utf8.size()))
                .ToLocalChecked()
          : ToV8String(env, source->text, "");
  v8::Local<v8::String> filename_str = ToV8String(env, filename, "[eval]");

  v8::Local<v8::Symbol> host_id_symbol;
  if (host_defined_option_id != nullptr) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }

  v8::TryCatch try_catch(isolate);
  v8::Context::Scope scope(target_context);
  v8::ScriptOrigin origin(filename_str,
                          line_offset,
                          column_offset,
                          true,
                          -1,
                          v8::Local<v8::Value>(),
                          false,
                          false,
                          false,
                          HostDefinedOptions(isolate, host_id_symbol));
  v8::ScriptCompiler::CachedData* run_cached_data = nullptr;
  if (bytecode_record != nullptr && !bytecode_record->bytes.empty()) {
    run_cached_data = new v8::ScriptCompiler::CachedData(
        bytecode_record->bytes.data(), static_cast<int>(bytecode_record->bytes.size()),
        v8::ScriptCompiler::CachedData::BufferNotOwned);
  }
  v8::ScriptCompiler::Source source_obj(code, origin, run_cached_data);
  v8::Local<v8::UnboundScript> unbound_script;
  if (!v8::ScriptCompiler::CompileUnboundScript(isolate,
                                                &source_obj,
                                                run_cached_data != nullptr
                                                    ? v8::ScriptCompiler::kConsumeCodeCache
                                                    : v8::ScriptCompiler::kNoCompileOptions,
                                                v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
           .ToLocal(&unbound_script)) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      if (display_errors && !try_catch.Message().IsEmpty()) {
        unofficial_napi_internal::AttachSyntaxArrowMessage(
            isolate, target_context, try_catch.Exception(), try_catch.Message());
      }
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_generic_failure;
  }

  v8::Local<v8::Script> script = unbound_script->BindToCurrentContext();
  bool timed_out = false;
  bool received_signal = false;
  v8::MaybeLocal<v8::Value> maybe_result;
  {
    std::unique_ptr<Watchdog> timeout_watchdog;
    if (timeout != -1) {
      timeout_watchdog = std::make_unique<Watchdog>(isolate, static_cast<uint64_t>(timeout), &timed_out);
    }

    std::unique_ptr<SigintWatchdog> sigint_watchdog;
    if (break_on_sigint) {
      sigint_watchdog = std::make_unique<SigintWatchdog>(isolate, &received_signal);
    }

    maybe_result = script->Run(target_context);
    if (!maybe_result.IsEmpty() && target_queue != nullptr) {
      target_queue->PerformCheckpoint(isolate);
    }
  }

  if (timed_out || received_signal) {
    isolate->CancelTerminateExecution();

    napi_value error = nullptr;
    if (timed_out) {
      CreateCodeError(env,
                      "ERR_SCRIPT_EXECUTION_TIMEOUT",
                      "Script execution timed out after " + std::to_string(timeout) + "ms",
                      &error);
    } else {
      CreateCodeError(
          env, "ERR_SCRIPT_EXECUTION_INTERRUPTED", "Script execution was interrupted by `SIGINT`", &error);
    }

    if (error != nullptr) {
      return napi_throw(env, error);
    }
    return napi_generic_failure;
  }

  if (try_catch.HasCaught()) {
    if (display_errors && !try_catch.HasTerminated() && !try_catch.Message().IsEmpty()) {
      unofficial_napi_internal::AttachSyntaxArrowMessage(
          isolate, target_context, try_catch.Exception(), try_catch.Message());
    }
    if (!try_catch.HasTerminated()) {
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_pending_exception;
  }

  v8::Local<v8::Value> result;
  if (!maybe_result.ToLocal(&result)) {
    return napi_generic_failure;
  }

  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(result));
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_dispose_context(
    napi_env env,
    napi_value sandbox_or_context_global) {
  if (env == nullptr || sandbox_or_context_global == nullptr) return napi_invalid_arg;

  std::lock_guard<std::mutex> lock(g_context_mu);
  auto it = g_context_records.find(env);
  if (it == g_context_records.end()) return napi_ok;

  auto& records = it->second;
  for (size_t i = 0; i < records.size(); ++i) {
    ContextRecord& rec = records[i];
    if (rec.key_ref == nullptr) continue;
    napi_value candidate = nullptr;
    if (napi_get_reference_value(env, rec.key_ref, &candidate) != napi_ok || candidate == nullptr) continue;
    bool same = false;
    if (napi_strict_equals(env, candidate, sandbox_or_context_global, &same) != napi_ok || !same) continue;
    if (env->context_token_unassign_callback != nullptr) {
      env->context_token_unassign_callback(
          env, rec.key_ref, env->context_token_callback_data);
    }
    napi_delete_reference(env, rec.key_ref);
    rec.key_ref = nullptr;
    rec.context.Reset();
    rec.own_microtask_queue.reset();
    records.erase(records.begin() + static_cast<long>(i));
    break;
  }

  if (records.empty()) {
    g_context_records.erase(it);
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_compile_function(
    napi_env env,
    const unofficial_napi_js_source* source,
    napi_value filename,
    int32_t line_offset,
    int32_t column_offset,
    napi_value parsing_context_or_undefined,
    napi_value context_extensions_or_undefined,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    napi_value* result_out) {
  if (env == nullptr || source == nullptr ||
      (source->text == nullptr && source->bytecode == nullptr) || filename == nullptr ||
      result_out == nullptr) {
    return napi_invalid_arg;
  }

  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> current = env->context();
  v8::Context::Scope current_scope(current);

  v8::Local<v8::Context> parsing_context = current;
  if (!IsNullish(env, parsing_context_or_undefined)) {
    std::lock_guard<std::mutex> lock(g_context_mu);
    v8::MicrotaskQueue* ignored = nullptr;
    if (!ResolveContextFromKey(env, parsing_context_or_undefined, &parsing_context, &ignored)) {
      return napi_invalid_arg;
    }
  }

  BytecodeRecord* bytecode_record = BytecodeRecordFromSource(source);
  if (bytecode_record != nullptr &&
      bytecode_record->shape != unofficial_napi_bytecode_shape_cjs_function) {
    return napi_invalid_arg;
  }

  std::string source_text = bytecode_record != nullptr ? bytecode_record->source_utf8
                                                       : ToUtf8String(env, source->text, "");
  if (StartsWithBomHashbang(source_text)) {
    v8::Local<v8::String> message = v8::String::NewFromUtf8Literal(
        isolate, "Invalid or unexpected token");
    v8::Local<v8::Value> exception = v8::Exception::SyntaxError(message);
    napi_value wrapped_exception = napi_v8_wrap_value(env, exception);
    if (wrapped_exception != nullptr) {
      (void)napi_throw(env, wrapped_exception);
    } else {
      isolate->ThrowException(exception);
    }
    return napi_pending_exception;
  }

  v8::Local<v8::String> filename_str = ToV8String(env, filename, "");

  v8::Local<v8::Symbol> host_id_symbol;
  if (host_defined_option_id != nullptr) {
    v8::Local<v8::Value> host_raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!host_raw.IsEmpty() && host_raw->IsSymbol()) {
      host_id_symbol = host_raw.As<v8::Symbol>();
    }
  }

  std::vector<v8::Local<v8::Object>> context_extensions;
  if (!IsNullish(env, context_extensions_or_undefined)) {
    v8::Local<v8::Value> value = napi_v8_unwrap_value(context_extensions_or_undefined);
    if (value.IsEmpty() || !value->IsArray()) return napi_invalid_arg;
    v8::Local<v8::Array> array = value.As<v8::Array>();
    context_extensions.reserve(array->Length());
    for (uint32_t i = 0; i < array->Length(); ++i) {
      v8::Local<v8::Value> item;
      if (!array->Get(current, i).ToLocal(&item) || !item->IsObject()) return napi_invalid_arg;
      context_extensions.push_back(item.As<v8::Object>());
    }
  }

  std::vector<std::string> param_strings;
  if (!ReadParamsArray(env, params_or_undefined, &param_strings)) return napi_invalid_arg;

  // Artifact fast path: bytecode_compile/deserialize already built this exact
  // function (same context, offsets, params, filename, host symbol) — reuse
  // it instead of paying a second code-cache consume.
  v8::Local<v8::Function> fn;
  v8::Local<v8::Symbol> effective_symbol = host_id_symbol;
  bool consumed_cached_data = false;
  bool cached_data_rejected = false;
  if (bytecode_record != nullptr && !bytecode_record->function.IsEmpty() &&
      context_extensions.empty() &&
      bytecode_record->function_context.Get(isolate) == parsing_context &&
      bytecode_record->line_offset == line_offset &&
      bytecode_record->column_offset == column_offset &&
      bytecode_record->params == param_strings &&
      bytecode_record->filename_utf8 == ToUtf8String(env, filename, "")) {
    v8::Local<v8::Symbol> record_symbol;
    if (!bytecode_record->host_symbol.IsEmpty()) {
      record_symbol = bytecode_record->host_symbol.Get(isolate);
    }
    // bytecode_compile/deserialize default CJS-function records to the
    // loader's dynamic-import symbol; a caller passing no symbol gets the
    // artifact it asked those APIs to build.
    const bool host_symbol_matches =
        record_symbol == host_id_symbol ||
        (host_id_symbol.IsEmpty() && !record_symbol.IsEmpty() &&
         record_symbol == DefaultCjsHostSymbol(env));
    if (host_symbol_matches) {
      fn = bytecode_record->function.Get(isolate);
      effective_symbol = record_symbol;
    }
  }

  if (fn.IsEmpty()) {
    v8::Local<v8::String> code_str =
        v8::String::NewFromUtf8(env->isolate,
                                source_text.c_str(),
                                v8::NewStringType::kNormal,
                                static_cast<int>(source_text.size()))
            .ToLocalChecked();

    v8::ScriptCompiler::CachedData* cached_data = nullptr;
    if (bytecode_record != nullptr && !bytecode_record->bytes.empty()) {
      cached_data = new v8::ScriptCompiler::CachedData(
          bytecode_record->bytes.data(), static_cast<int>(bytecode_record->bytes.size()),
          v8::ScriptCompiler::CachedData::BufferNotOwned);
    }

    std::vector<v8::Local<v8::String>> params = ToV8Params(isolate, param_strings);

    v8::ScriptOrigin origin(filename_str,
                            line_offset,
                            column_offset,
                            true,
                            -1,
                            v8::Local<v8::Value>(),
                            false,
                            false,
                            false,
                            HostDefinedOptions(isolate, host_id_symbol));

    v8::ScriptCompiler::Source source_obj(code_str, origin, cached_data);
    v8::ScriptCompiler::CompileOptions options = source_obj.GetCachedData() != nullptr
                                                     ? v8::ScriptCompiler::kConsumeCodeCache
                                                     : v8::ScriptCompiler::kNoCompileOptions;

    v8::TryCatch try_catch(isolate);
    v8::Context::Scope parsing_scope(parsing_context);
    v8::MaybeLocal<v8::Function> maybe_fn = v8::ScriptCompiler::CompileFunction(
        parsing_context,
        &source_obj,
        params.size(),
        params.empty() ? nullptr : params.data(),
        context_extensions.size(),
        context_extensions.empty() ? nullptr : context_extensions.data(),
        options,
        v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason);

    if (!maybe_fn.ToLocal(&fn)) {
      if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
        if (!try_catch.Message().IsEmpty()) {
          unofficial_napi_internal::AttachSyntaxArrowMessage(
              isolate, parsing_context, try_catch.Exception(), try_catch.Message());
        }
        return ThrowTryCatchException(env, try_catch);
      }
      return napi_generic_failure;
    }

    consumed_cached_data =
        options == v8::ScriptCompiler::kConsumeCodeCache && source_obj.GetCachedData() != nullptr;
    if (consumed_cached_data) {
      cached_data_rejected = source_obj.GetCachedData()->rejected;
    }
  }

  SetApiPrivate(current,
                fn.As<v8::Object>(),
                "node:host_defined_option_symbol",
                effective_symbol.IsEmpty() ? v8::Undefined(isolate) : effective_symbol.As<v8::Value>());

  v8::Local<v8::Object> out = v8::Object::New(isolate);
  if (!SetNamed(current, out, "function", fn)) return napi_generic_failure;

  v8::ScriptOrigin fn_origin = fn->GetScriptOrigin();
  if (!SetNamed(current, out, "sourceURL", fn_origin.ResourceName())) return napi_generic_failure;
  if (!SetNamed(current, out, "sourceMapURL", fn_origin.SourceMapUrl())) return napi_generic_failure;

  if (consumed_cached_data) {
    if (!SetNamed(current,
                  out,
                  "cachedDataRejected",
                  v8::Boolean::New(isolate, cached_data_rejected))) {
      return napi_generic_failure;
    }
  }

  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(out));
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_compile_function_for_cjs_loader(
    napi_env env,
    const unofficial_napi_js_source* source,
    napi_value filename,
    bool is_sea_main,
    bool should_detect_module,
    napi_value* result_out) {
  if (env == nullptr || source == nullptr ||
      (source->text == nullptr && source->bytecode == nullptr) || filename == nullptr ||
      result_out == nullptr) {
    return napi_invalid_arg;
  }
  (void)is_sea_main;

  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  BytecodeRecord* bytecode_record = BytecodeRecordFromSource(source);
  if (bytecode_record != nullptr &&
      bytecode_record->shape != unofficial_napi_bytecode_shape_cjs_function) {
    return napi_invalid_arg;
  }

  v8::Local<v8::String> code_str =
      bytecode_record != nullptr
          ? v8::String::NewFromUtf8(isolate, bytecode_record->source_utf8.c_str(),
                                    v8::NewStringType::kNormal,
                                    static_cast<int>(bytecode_record->source_utf8.size()))
                .ToLocalChecked()
          : ToV8String(env, source->text, "");
  v8::Local<v8::String> filename_str = ToV8String(env, filename, "[eval]");
  v8::Local<v8::Symbol> host_id_symbol = DefaultCjsHostSymbol(env);

  v8::Local<v8::Function> fn;
  v8::Local<v8::Value> cjs_exception;
  v8::Local<v8::Message> cjs_message;
  bool cjs_ok = false;

  if (bytecode_record != nullptr) {
    // Fast path: bytecode_compile/deserialize built the artifact in this env
    // context with the same loader host symbol — reuse it directly.
    if (!bytecode_record->function.IsEmpty() &&
        bytecode_record->function_context.Get(isolate) == context) {
      fn = bytecode_record->function.Get(isolate);
      cjs_ok = true;
    } else if (!bytecode_record->bytes.empty()) {
      v8::TryCatch tc(isolate);
      auto* cached_data = new v8::ScriptCompiler::CachedData(
          bytecode_record->bytes.data(), static_cast<int>(bytecode_record->bytes.size()),
          v8::ScriptCompiler::CachedData::BufferNotOwned);
      v8::ScriptOrigin origin = BytecodeOrigin(isolate, filename_str, *bytecode_record, host_id_symbol);
      v8::ScriptCompiler::Source source_obj(code_str, origin, cached_data);
      std::vector<v8::Local<v8::String>> params = ToV8Params(isolate, bytecode_record->params);
      cjs_ok = v8::ScriptCompiler::CompileFunction(context,
                                                   &source_obj,
                                                   params.size(),
                                                   params.empty() ? nullptr : params.data(),
                                                   0,
                                                   nullptr,
                                                   v8::ScriptCompiler::kConsumeCodeCache,
                                                   v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
                   .ToLocal(&fn);
      // A failure here is unexpected (the retained source is valid); fall
      // through to a plain text compile below.
    }
  }

  if (!cjs_ok) {
    v8::TryCatch tc(isolate);
    cjs_ok = CompileCjsFunction(context, code_str, filename_str, true, host_id_symbol).ToLocal(&fn);
    if (!cjs_ok && tc.HasCaught()) {
      cjs_exception = tc.Exception();
      cjs_message = tc.Message();
    }
  }

  bool can_parse_as_esm = false;
  if (!cjs_ok) {
    if (!cjs_message.IsEmpty()) {
      can_parse_as_esm =
          ShouldRetryAsEsm(isolate, context, env, cjs_message->Get(), code_str, filename_str);
    }
    if (!can_parse_as_esm || !should_detect_module) {
      if (!cjs_exception.IsEmpty()) {
        unofficial_napi_internal::AttachSyntaxArrowMessage(isolate, context, cjs_exception, cjs_message);
        isolate->ThrowException(cjs_exception);
      }
      return cjs_exception.IsEmpty() ? napi_generic_failure : napi_pending_exception;
    }
  }

  v8::Local<v8::Object> out = v8::Object::New(isolate);
  if (!SetNamed(context, out, "cachedDataRejected", v8::Boolean::New(isolate, false)) ||
      !SetNamed(context, out, "canParseAsESM", v8::Boolean::New(isolate, can_parse_as_esm))) {
    return napi_generic_failure;
  }

  if (cjs_ok) {
    if (!host_id_symbol.IsEmpty()) {
      SetApiPrivate(context, fn.As<v8::Object>(), "node:host_defined_option_symbol", host_id_symbol.As<v8::Value>());
    }
    v8::ScriptOrigin origin = fn->GetScriptOrigin();
    if (!SetNamed(context, out, "sourceMapURL", origin.SourceMapUrl()) ||
        !SetNamed(context, out, "sourceURL", origin.ResourceName()) ||
        !SetNamed(context, out, "function", fn)) {
      return napi_generic_failure;
    }
  } else {
    if (!SetNamed(context, out, "sourceMapURL", v8::Undefined(isolate)) ||
        !SetNamed(context, out, "sourceURL", v8::Undefined(isolate)) ||
        !SetNamed(context, out, "function", v8::Undefined(isolate))) {
      return napi_generic_failure;
    }
  }

  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(out));
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_contains_module_syntax(
    napi_env env,
    napi_value code,
    napi_value filename,
    napi_value resource_name_or_undefined,
    bool cjs_var_in_scope,
    bool* result_out) {
  if (env == nullptr || code == nullptr || filename == nullptr || result_out == nullptr) return napi_invalid_arg;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  v8::Local<v8::String> code_str = ToV8String(env, code, "");
  v8::Local<v8::String> filename_str = ToV8String(env, filename, "[eval]");
  v8::Local<v8::String> resource_name = filename_str;
  if (!IsNullish(env, resource_name_or_undefined)) {
    resource_name = ToV8String(env, resource_name_or_undefined, "[eval]");
  }

  {
    v8::TryCatch tc(isolate);
    v8::Local<v8::Function> fn;
    if (CompileCjsFunction(context, code_str, filename_str, cjs_var_in_scope).ToLocal(&fn)) {
      *result_out = false;
      return napi_ok;
    }
    if (tc.HasCaught()) {
      *result_out = ShouldRetryAsEsm(isolate, context, env, tc.Message()->Get(), code_str, resource_name);
      return napi_ok;
    }
  }
  *result_out = false;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_start_sigint_watchdog(
    napi_env env,
    bool* result_out) {
  if (env == nullptr || result_out == nullptr) return napi_invalid_arg;
  std::lock_guard<std::mutex> action_lock(SigintWatchdogHelper::GetActionMutex());
  *result_out = SigintWatchdogHelper::GetInstance().Start() == 0;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_stop_sigint_watchdog(
    napi_env env,
    bool* had_pending_signal_out) {
  if (env == nullptr || had_pending_signal_out == nullptr) return napi_invalid_arg;
  std::lock_guard<std::mutex> action_lock(SigintWatchdogHelper::GetActionMutex());
  *had_pending_signal_out = SigintWatchdogHelper::GetInstance().Stop();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_contextify_watchdog_has_pending_sigint(
    napi_env env,
    bool* result_out) {
  if (env == nullptr || result_out == nullptr) return napi_invalid_arg;
  *result_out = SigintWatchdogHelper::GetInstance().HasPendingSignal();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_create_source_text(
    napi_env env,
    napi_value wrapper,
    napi_value url,
    napi_value context_or_undefined,
    const unofficial_napi_js_source* source,
    int32_t line_offset,
    int32_t column_offset,
    napi_value host_defined_option_id,
    void** handle_out) {
  if (env == nullptr || wrapper == nullptr || url == nullptr || source == nullptr ||
      (source->text == nullptr && source->bytecode == nullptr) || handle_out == nullptr) {
    return napi_invalid_arg;
  }
  *handle_out = nullptr;

  EnsureModuleWrapCleanupHook(env);
  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context =
      !IsNullish(env, context_or_undefined) && napi_v8_unwrap_value(context_or_undefined)->IsObject()
          ? napi_v8_unwrap_value(context_or_undefined).As<v8::Object>()->GetCreationContextChecked()
          : env->context();
  v8::Context::Scope context_scope(context);

  BytecodeRecord* bytecode_record = BytecodeRecordFromSource(source);
  if (bytecode_record != nullptr &&
      bytecode_record->shape != unofficial_napi_bytecode_shape_module) {
    return napi_invalid_arg;
  }

  v8::Local<v8::String> url_str = ToV8String(env, url, "vm:module");
  v8::Local<v8::String> source_str =
      bytecode_record != nullptr
          ? v8::String::NewFromUtf8(isolate, bytecode_record->source_utf8.c_str(),
                                    v8::NewStringType::kNormal,
                                    static_cast<int>(bytecode_record->source_utf8.size()))
                .ToLocalChecked()
          : ToV8String(env, source->text, "");

  v8::Local<v8::Symbol> host_id_symbol;
  if (!IsNullish(env, host_defined_option_id)) {
    v8::Local<v8::Value> raw = napi_v8_unwrap_value(host_defined_option_id);
    if (!raw.IsEmpty() && raw->IsSymbol()) {
      host_id_symbol = raw.As<v8::Symbol>();
    }
  }
  if (host_id_symbol.IsEmpty()) {
    host_id_symbol = v8::Symbol::New(isolate, url_str);
  }

  v8::Local<v8::Module> module;
  const bool artifact_reusable =
      bytecode_record != nullptr && !bytecode_record->module.IsEmpty() &&
      context == env->context() && !host_id_symbol.IsEmpty() &&
      !bytecode_record->host_symbol.IsEmpty() &&
      bytecode_record->host_symbol.Get(isolate) == host_id_symbol;
  if (artifact_reusable) {
    // The artifact was compiled with the same host-defined-option symbol in
    // this context — use it directly (no second cache deserialize).
    module = bytecode_record->module.Get(isolate);
  } else {
    v8::ScriptCompiler::CachedData* cached_data = nullptr;
    if (bytecode_record != nullptr && !bytecode_record->bytes.empty()) {
      cached_data = new v8::ScriptCompiler::CachedData(
          bytecode_record->bytes.data(), static_cast<int>(bytecode_record->bytes.size()),
          v8::ScriptCompiler::CachedData::BufferNotOwned);
    }

    v8::ScriptOrigin origin(url_str,
                            line_offset,
                            column_offset,
                            true,
                            -1,
                            v8::Local<v8::Value>(),
                            false,
                            false,
                            true,
                            HostDefinedOptions(isolate, host_id_symbol));
    v8::ScriptCompiler::Source source_obj(source_str, origin, cached_data);
    v8::TryCatch try_catch(isolate);
    if (!v8::ScriptCompiler::CompileModule(isolate,
                                           &source_obj,
                                           cached_data != nullptr
                                               ? v8::ScriptCompiler::kConsumeCodeCache
                                               : v8::ScriptCompiler::kNoCompileOptions,
                                           v8::ScriptCompiler::NoCacheReason::kNoCacheNoReason)
             .ToLocal(&module)) {
      if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
        unofficial_napi_internal::AttachSyntaxArrowMessage(
            isolate, context, try_catch.Exception(), try_catch.Message());
        return ThrowTryCatchException(env, try_catch);
      }
      return napi_generic_failure;
    }
  }

  auto* record = env->allocate<ModuleWrapRecord>();
  record->env = env;
  record->context.Reset(isolate, context);
  record->module.Reset(isolate, module);
  napi_create_reference(env, wrapper, 1, &record->wrapper_ref);

  napi_value host_id_value = napi_v8_wrap_value(env, host_id_symbol);
  if (host_id_value != nullptr) {
    napi_create_reference(env, host_id_value, 1, &record->host_defined_option_ref);
    (void)SetHostDefinedOptionSymbolOnWrapper(env, wrapper, host_id_value);
  }

  napi_value has_tla = napi_v8_wrap_value(env, v8::Boolean::New(isolate, module->HasTopLevelAwait()));
  if (has_tla != nullptr) {
    (void)napi_set_named_property(env, wrapper, "hasTopLevelAwait", has_tla);
  }
  napi_value source_url = napi_v8_wrap_value(env, module->GetUnboundModuleScript()->GetSourceURL());
  if (source_url != nullptr) {
    (void)napi_set_named_property(env, wrapper, "sourceURL", source_url);
  }
  napi_value source_map_url = napi_v8_wrap_value(env, module->GetUnboundModuleScript()->GetSourceMappingURL());
  if (source_map_url != nullptr) {
    (void)napi_set_named_property(env, wrapper, "sourceMapURL", source_map_url);
  }

  if (!PopulateModuleRequests(env, record, context, module)) {
    DestroyModuleRecord(record);
    return napi_generic_failure;
  }

  {
    std::lock_guard<std::mutex> lock(g_module_wrap_mu);
    GetModuleWrapState(env)->modules.push_back(record);
  }
  *handle_out = record;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_create_synthetic(
    napi_env env,
    napi_value wrapper,
    napi_value url,
    napi_value context_or_undefined,
    napi_value export_names,
    napi_value synthetic_eval_steps,
    void** handle_out) {
  if (env == nullptr || wrapper == nullptr || url == nullptr || export_names == nullptr || synthetic_eval_steps == nullptr ||
      handle_out == nullptr) {
    return napi_invalid_arg;
  }
  *handle_out = nullptr;

  bool is_array = false;
  if (napi_is_array(env, export_names, &is_array) != napi_ok || !is_array) return napi_invalid_arg;

  EnsureModuleWrapCleanupHook(env);
  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context =
      !IsNullish(env, context_or_undefined) && napi_v8_unwrap_value(context_or_undefined)->IsObject()
          ? napi_v8_unwrap_value(context_or_undefined).As<v8::Object>()->GetCreationContextChecked()
          : env->context();
  v8::Context::Scope context_scope(context);

  uint32_t export_count = 0;
  napi_get_array_length(env, export_names, &export_count);
  std::vector<v8::Local<v8::String>> export_names_v8;
  export_names_v8.reserve(export_count);
  for (uint32_t i = 0; i < export_count; ++i) {
    napi_value export_name = nullptr;
    if (napi_get_element(env, export_names, i, &export_name) != napi_ok || export_name == nullptr) {
      return napi_invalid_arg;
    }
    v8::Local<v8::Value> raw = napi_v8_unwrap_value(export_name);
    if (raw.IsEmpty() || !raw->IsString()) return napi_invalid_arg;
    export_names_v8.push_back(raw.As<v8::String>());
  }

  v8::MemorySpan<const v8::Local<v8::String>> names_span(export_names_v8.data(), export_names_v8.size());
  v8::Local<v8::Module> module = v8::Module::CreateSyntheticModule(
      isolate, ToV8String(env, url, "vm:synthetic"), names_span, SyntheticModuleEvaluationSteps);

  auto* record = env->allocate<ModuleWrapRecord>();
  record->env = env;
  record->context.Reset(isolate, context);
  record->module.Reset(isolate, module);
  napi_create_reference(env, wrapper, 1, &record->wrapper_ref);
  napi_create_reference(env, synthetic_eval_steps, 1, &record->synthetic_eval_steps_ref);

  {
    std::lock_guard<std::mutex> lock(g_module_wrap_mu);
    GetModuleWrapState(env)->modules.push_back(record);
  }
  *handle_out = record;
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_destroy(napi_env env, void* handle) {
  if (env == nullptr || handle == nullptr) return napi_invalid_arg;
  {
    std::lock_guard<std::mutex> lock(g_module_wrap_mu);
    auto it = g_module_wrap_states.find(env);
    if (it == g_module_wrap_states.end()) return napi_ok;
    auto& modules = it->second.modules;
    if (std::find(modules.begin(), modules.end(), static_cast<ModuleWrapRecord*>(handle)) == modules.end()) {
      return napi_ok;
    }
  }
  DestroyModuleRecord(static_cast<ModuleWrapRecord*>(handle));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_get_module_requests(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  std::vector<v8::Local<v8::Value>> requests;
  requests.reserve(record->module_requests.size());
  for (uint32_t i = 0; i < record->module_requests.size(); ++i) {
    const auto& request_info = record->module_requests[i];
    std::vector<v8::Local<v8::Name>> attribute_names;
    std::vector<v8::Local<v8::Value>> attribute_values;
    attribute_names.reserve(request_info.attributes.size());
    attribute_values.reserve(request_info.attributes.size());
    for (const auto& attr : request_info.attributes) {
      attribute_names.push_back(OneByteString(isolate, attr.key.c_str()));
      attribute_values.push_back(OneByteString(isolate, attr.value.c_str()));
    }
    v8::Local<v8::Object> attributes = CreateFrozenNullProtoObject(env, attribute_names, attribute_values);

    std::vector<v8::Local<v8::Name>> request_names;
    std::vector<v8::Local<v8::Value>> request_values;
    request_names.reserve(3);
    request_values.reserve(3);
    request_names.push_back(OneByteString(isolate, "specifier"));
    request_values.push_back(OneByteString(isolate, request_info.specifier.c_str()));
    request_names.push_back(OneByteString(isolate, "attributes"));
    request_values.push_back(attributes);
    request_names.push_back(OneByteString(isolate, "phase"));
    request_values.push_back(v8::Integer::New(isolate, request_info.phase));

    requests.push_back(CreateFrozenNullProtoObject(env, request_names, request_values));
  }

  v8::Local<v8::Array> result = v8::Array::New(isolate, requests.data(), requests.size());
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(result));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_link(
    napi_env env,
    void* handle,
    size_t count,
    void* const* linked_handles) {
  if (env == nullptr || handle == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  if (count != record->module_requests.size()) {
    ThrowCodeError(env, "ERR_VM_MODULE_LINK_FAILURE", "linked modules array length mismatch");
    return napi_pending_exception;
  }

  record->linked_requests.assign(count, nullptr);
  for (size_t i = 0; i < count; ++i) {
    ModuleWrapRecord* linked = linked_handles != nullptr ? static_cast<ModuleWrapRecord*>(linked_handles[i]) : nullptr;
    if (linked == nullptr) {
      ThrowCodeError(env, "ERR_VM_MODULE_LINK_FAILURE", "linked module missing");
      return napi_pending_exception;
    }
    record->linked_requests[i] = linked;
    const std::string key =
        SerializeModuleRequestKey(record->module_requests[i].specifier, record->module_requests[i].attributes);
    auto it = record->resolve_cache.find(key);
    if (it != record->resolve_cache.end() && it->second < i && record->linked_requests[it->second] != linked) {
      ThrowCodeError(env,
                     "ERR_MODULE_LINK_MISMATCH",
                     "Module request '" + record->module_requests[i].specifier + "' must be linked to the same module");
      return napi_pending_exception;
    }
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_instantiate(napi_env env, void* handle) {
  if (env == nullptr || handle == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = record->context.Get(isolate);
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Module> module = record->module.Get(isolate);

  v8::TryCatch try_catch(isolate);
  auto maybe = module->InstantiateModule(context, ModuleResolveCallback, ModuleResolveSourceCallback);
  if (maybe.IsNothing()) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_evaluate(
    napi_env env,
    void* handle,
    int64_t /*timeout*/,
    bool /*break_on_sigint*/,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = record->context.Get(isolate);
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Module> module = record->module.Get(isolate);

  v8::TryCatch try_catch(isolate);
  v8::Local<v8::Value> result;
  if (!module->Evaluate(context).ToLocal(&result)) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_generic_failure;
  }
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(result));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_evaluate_sync(
    napi_env env,
    void* handle,
    napi_value filename,
    napi_value parent_filename,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  *result_out = nullptr;

  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = record->context.Get(isolate);
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Module> module = record->module.Get(isolate);

  v8::TryCatch try_catch(isolate);
  v8::Local<v8::Value> result;
  if (!module->Evaluate(context).ToLocal(&result)) {
    if (try_catch.HasCaught() && !try_catch.HasTerminated()) {
      return ThrowTryCatchException(env, try_catch);
    }
    return napi_generic_failure;
  }
  if (!result->IsPromise()) return napi_generic_failure;

  napi_value promise = napi_v8_wrap_value(env, result);
  if (promise == nullptr) return napi_generic_failure;

  int32_t promise_state = 0;
  napi_value promise_result = nullptr;
  bool has_promise_result = false;
  napi_status status = unofficial_napi_get_promise_details(
      env, promise, &promise_state, &promise_result, &has_promise_result);
  if (status != napi_ok) return status;

  if (promise_state == 2 && has_promise_result && promise_result != nullptr) {
    status = unofficial_napi_mark_promise_as_handled(env, promise);
    if (status != napi_ok && status != napi_pending_exception) return status;
    napi_throw(env, promise_result);
    return napi_pending_exception;
  }

  if (module->IsGraphAsync()) {
    auto stalled_messages = std::get<1>(module->GetStalledTopLevelAwaitMessages(isolate));
    for (const auto& message : stalled_messages) {
      const std::string info = unofficial_napi_internal::BuildSyntaxArrowMessage(isolate, context, message);
      std::fprintf(stderr, "Error: unexpected top-level await at %s\n", info.c_str());
    }
    ThrowCodeError(env,
                   "ERR_REQUIRE_ASYNC_MODULE",
                   RequireAsyncModuleMessage(
                       filename != nullptr ? V8ValueToUtf8(isolate, napi_v8_unwrap_value(filename)) : "",
                       parent_filename != nullptr
                           ? V8ValueToUtf8(isolate, napi_v8_unwrap_value(parent_filename))
                           : ""));
    return napi_pending_exception;
  }

  if (promise_state != 1) return napi_generic_failure;

  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(module->GetModuleNamespace()));
  return *result_out != nullptr ? napi_ok : napi_generic_failure;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_get_namespace(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = record->context.Get(isolate);
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Module> module = record->module.Get(isolate);
  if (module->GetStatus() < v8::Module::Status::kInstantiated) {
    ThrowCodeError(env, "ERR_MODULE_NOT_INSTANTIATED", "Module is not instantiated");
    return napi_pending_exception;
  }
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(module->GetModuleNamespace()));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_get_status(
    napi_env env,
    void* handle,
    int32_t* status_out) {
  if (env == nullptr || handle == nullptr || status_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  *status_out = static_cast<int32_t>(record->module.Get(env->isolate)->GetStatus());
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_get_error(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(record->module.Get(isolate)->GetException()));
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_has_top_level_await(
    napi_env env,
    void* handle,
    bool* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  *result_out = record->module.Get(env->isolate)->HasTopLevelAwait();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_has_async_graph(
    napi_env env,
    void* handle,
    bool* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Local<v8::Module> module = record->module.Get(env->isolate);
  if (module->GetStatus() < v8::Module::Status::kInstantiated) {
    ThrowCodeError(env, "ERR_MODULE_NOT_INSTANTIATED", "Module is not instantiated");
    return napi_pending_exception;
  }
  *result_out = module->IsGraphAsync();
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_check_unsettled_top_level_await(
    napi_env env,
    napi_value module_wrap,
    bool warnings,
    bool* settled_out) {
  if (env == nullptr || settled_out == nullptr) return napi_invalid_arg;
  *settled_out = true;
  if (module_wrap == nullptr) return napi_ok;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, module_wrap, &type) != napi_ok || type != napi_object) {
    return napi_ok;
  }

  ModuleWrapRecord* record = FindModuleRecordForWrapper(env, module_wrap);
  if (record == nullptr) return napi_ok;

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);
  v8::Local<v8::Module> module = record->module.Get(isolate);

  if (!module->IsSourceTextModule()) return napi_ok;
  if (!module->IsGraphAsync()) return napi_ok;

  auto stalled_messages = std::get<1>(module->GetStalledTopLevelAwaitMessages(isolate));
  if (stalled_messages.empty()) return napi_ok;

  *settled_out = false;
  if (!warnings) return napi_ok;

  for (const auto& message : stalled_messages) {
    const std::string info = unofficial_napi_internal::BuildSyntaxArrowMessage(isolate, context, message);
    std::fprintf(stderr, "Warning: Detected unsettled top-level await at %s\n", info.c_str());
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_set_export(
    napi_env env,
    void* handle,
    napi_value export_name,
    napi_value export_value) {
  if (env == nullptr || handle == nullptr || export_name == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Local<v8::Value> name = napi_v8_unwrap_value(export_name);
  v8::Local<v8::Value> value =
      export_value != nullptr ? napi_v8_unwrap_value(export_value) : v8::Undefined(env->isolate).As<v8::Value>();
  if (name.IsEmpty() || !name->IsString()) return napi_invalid_arg;
  if (record->module.Get(env->isolate)->SetSyntheticModuleExport(env->isolate, name.As<v8::String>(), value).IsNothing()) {
    return napi_generic_failure;
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_set_module_source_object(
    napi_env env,
    void* handle,
    napi_value source_object) {
  if (env == nullptr || handle == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  ResetRef(env, &record->source_object_ref);
  if (source_object != nullptr) {
    napi_create_reference(env, source_object, 1, &record->source_object_ref);
  }
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_get_module_source_object(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  *result_out = GetRefValue(env, record->source_object_ref);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_create_cached_data(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Module> module = record->module.Get(isolate);
  napi_value out = nullptr;
  if (!module->IsSourceTextModule()) {
    if (!CreateNodeBufferFromBytes(env, nullptr, 0, &out) || out == nullptr) return napi_generic_failure;
  } else {
    std::unique_ptr<v8::ScriptCompiler::CachedData> cached_data(
        v8::ScriptCompiler::CreateCodeCache(module->GetUnboundModuleScript()));
    const uint8_t* bytes = cached_data ? cached_data->data : nullptr;
    const size_t size = cached_data ? static_cast<size_t>(cached_data->length) : 0;
    if (!CreateNodeBufferFromBytes(env, bytes, size, &out) || out == nullptr) return napi_generic_failure;
  }
  v8::Local<v8::Value> out_value = napi_v8_unwrap_value(out);
  if (out_value.IsEmpty()) return napi_generic_failure;
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(out_value));
  return *result_out != nullptr ? napi_ok : napi_generic_failure;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_set_import_module_dynamically_callback(
    napi_env env,
    napi_value callback) {
  if (env == nullptr) return napi_invalid_arg;
  EnsureModuleWrapCleanupHook(env);
  std::lock_guard<std::mutex> lock(g_module_wrap_mu);
  auto* state = GetModuleWrapState(env);
  ResetRef(env, &state->import_module_dynamically_ref);
  if (callback != nullptr) napi_create_reference(env, callback, 1, &state->import_module_dynamically_ref);
  env->isolate->SetHostImportModuleDynamicallyCallback(ImportModuleDynamically);
  env->isolate->SetHostImportModuleWithPhaseDynamicallyCallback(ImportModuleDynamicallyWithPhase);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_set_initialize_import_meta_object_callback(
    napi_env env,
    napi_value callback) {
  if (env == nullptr) return napi_invalid_arg;
  EnsureModuleWrapCleanupHook(env);
  std::lock_guard<std::mutex> lock(g_module_wrap_mu);
  auto* state = GetModuleWrapState(env);
  ResetRef(env, &state->initialize_import_meta_ref);
  if (callback != nullptr) napi_create_reference(env, callback, 1, &state->initialize_import_meta_ref);
  env->isolate->SetHostInitializeImportMetaObjectCallback(HostInitializeImportMetaObject);
  return napi_ok;
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_import_module_dynamically(
    napi_env env,
    size_t argc,
    napi_value* argv,
    napi_value* result_out) {
  if (env == nullptr || argv == nullptr || result_out == nullptr) return napi_invalid_arg;
  *result_out = nullptr;

  auto* state = GetModuleWrapState(env);
  if (state == nullptr) return napi_generic_failure;
  napi_value callback = GetRefValue(env, state->import_module_dynamically_ref);
  if (callback == nullptr) return napi_invalid_arg;

  napi_value global = nullptr;
  napi_get_global(env, &global);
  if (argc >= 5) {
    return napi_call_function(env, global, callback, 5, argv, result_out);
  }

  napi_value referrer_symbol = GetVmDynamicImportDefaultInternalSymbol(env);
  napi_value phase = nullptr;
  napi_create_int32(env, 2, &phase);
  std::vector<v8::Local<v8::Name>> empty_names;
  std::vector<v8::Local<v8::Value>> empty_values;
  napi_value attrs = napi_v8_wrap_value(env, CreateFrozenNullProtoObject(env, empty_names, empty_values));
  napi_value referrer_name = argc >= 2 ? argv[1] : nullptr;
  napi_value call_argv[5] = {referrer_symbol, argv[0], phase, attrs, referrer_name};
  return napi_call_function(env, global, callback, 5, call_argv, result_out);
}

napi_status NAPI_CDECL unofficial_napi_module_wrap_create_required_module_facade(
    napi_env env,
    void* handle,
    napi_value* result_out) {
  if (env == nullptr || handle == nullptr || result_out == nullptr) return napi_invalid_arg;
  ModuleWrapRecord* record = static_cast<ModuleWrapRecord*>(handle);
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope handle_scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Context::Scope context_scope(context);

  const char* kFacadeUrl = "node:internal/require_module_default_facade";
  const char* kFacadeSource = "export * from 'original'; export { default } from 'original'; export const __esModule = true;";
  v8::ScriptOrigin origin(OneByteString(isolate, kFacadeUrl),
                          0,
                          0,
                          true,
                          -1,
                          v8::Local<v8::Value>(),
                          false,
                          false,
                          true);
  v8::ScriptCompiler::Source source(OneByteString(isolate, kFacadeSource), origin);
  v8::Local<v8::Module> facade;
  if (!v8::ScriptCompiler::CompileModule(isolate, &source).ToLocal(&facade)) {
    return napi_pending_exception;
  }

  {
    std::lock_guard<std::mutex> lock(g_module_wrap_mu);
    GetModuleWrapState(env)->temporary_required_module_facade_original = record;
  }
  const bool instantiated = facade->InstantiateModule(context, LinkRequiredFacadeOriginal).FromMaybe(false);
  {
    std::lock_guard<std::mutex> lock(g_module_wrap_mu);
    GetModuleWrapState(env)->temporary_required_module_facade_original = nullptr;
  }
  if (!instantiated) return napi_pending_exception;

  v8::Local<v8::Value> evaluated;
  if (!facade->Evaluate(context).ToLocal(&evaluated)) return napi_pending_exception;
  *result_out = napi_v8_wrap_value(env, handle_scope.Escape(facade->GetModuleNamespace()));
  return napi_ok;
}

}  // extern "C"
