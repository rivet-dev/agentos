#include "internal_binding/binding_messaging.h"
#include "internal_binding/binding_initializers.h"

#include <algorithm>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <limits>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#include <uv.h>

#include "binding_registry/binding_registry.h"
#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"
#include "../edge_module_loader.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_runtime.h"
#include "edge_worker_env.h"

namespace internal_binding {

napi_value EdgeCryptoCreateNativeKeyObjectCloneData(napi_env env, napi_value value);
napi_value EdgeCryptoCreateKeyObjectFromCloneData(napi_env env, napi_value data);

struct MessagePort;
struct BroadcastChannelGroup;

struct Message {
  void* payload_data = nullptr;
  bool is_close = false;
  MessagePort* close_source = nullptr;
  struct TransferredPortData {
    napi_ref source_port_ref = nullptr;
    MessagePortDataPtr data;
  };
  using TransferredPortEntry = TransferredPortData;
  std::vector<TransferredPortData> transferred_ports;

  bool IsCloseMessage() const { return is_close; }
};

using QueuedMessage = Message;

struct MessagePortData {
  std::mutex mutex;
  std::weak_ptr<MessagePortData> sibling;
  std::shared_ptr<BroadcastChannelGroup> broadcast_group;
  std::deque<QueuedMessage> queued_messages;
  bool close_message_enqueued = false;
  bool closed = false;
  MessagePort* attached_port = nullptr;
};

struct BroadcastChannelGroup {
  explicit BroadcastChannelGroup(std::string group_name) : name(std::move(group_name)) {}

  std::mutex mutex;
  std::string name;
  std::vector<std::weak_ptr<MessagePortData>> members;
};

struct MessagePort {
  EdgeHandleWrap handle_wrap{};
  MessagePortDataPtr data;
  uv_async_t async{};
  int64_t async_id = 0;
  bool closing_has_ref = false;
  bool receiving_messages = false;
};

using MessagePortWrap = MessagePort;

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct MessagingState {
  explicit MessagingState(napi_env env_in) : env(env_in) {}
  ~MessagingState() {
    for (auto& entry : shared_handle_refs) {
      DeleteRefIfPresent(env, &entry.second);
    }
    shared_handle_refs.clear();
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &deserializer_create_object_ref);
    DeleteRefIfPresent(env, &emit_message_ref);
    DeleteRefIfPresent(env, &message_port_ctor_ref);
    DeleteRefIfPresent(env, &message_port_ctor_token_ref);
    DeleteRefIfPresent(env, &no_message_symbol_ref);
    DeleteRefIfPresent(env, &oninit_symbol_ref);
    DeleteRefIfPresent(env, &hybrid_dispatch_symbol_ref);
    DeleteRefIfPresent(env, &currently_receiving_ports_symbol_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref deserializer_create_object_ref = nullptr;
  napi_ref emit_message_ref = nullptr;
  napi_ref message_port_ctor_ref = nullptr;
  napi_ref message_port_ctor_token_ref = nullptr;
  napi_ref no_message_symbol_ref = nullptr;
  napi_ref oninit_symbol_ref = nullptr;
  napi_ref hybrid_dispatch_symbol_ref = nullptr;
  napi_ref currently_receiving_ports_symbol_ref = nullptr;
  uint32_t next_shared_handle_id = 1;
  std::unordered_map<uint32_t, napi_ref> shared_handle_refs;
};

constexpr napi_type_tag kMessagePortTypeTag = {
    0x0f0f1d6c1f8c4ce1ULL,
    0x9ab22b54f44c7dd3ULL,
};

std::mutex g_messaging_mu;
std::mutex g_broadcast_groups_mutex;
std::unordered_map<std::string, std::weak_ptr<BroadcastChannelGroup>> g_broadcast_groups;

napi_value ResolveDOMExceptionValue(napi_env env);
napi_value ResolveEmitMessageValue(napi_env env);
MessagePortWrap* UnwrapMessagePort(napi_env env, napi_value value);
MessagePortWrap* UnwrapMessagePortThisOrThrow(napi_env env, napi_value value);
napi_value MoveMessagePortPostMessageBridgeCallback(napi_env env, napi_callback_info info);
napi_value GetTransferListValue(napi_env env, napi_value value);
napi_value TryRequireModule(napi_env env, const char* module_name);
napi_value CreateInternalMessagePortInstance(napi_env env);
napi_value MoveMessagePortToContextCallback(napi_env env, napi_callback_info info);
bool IsArrayBufferLike(napi_env env, napi_value value);
bool IsMapValue(napi_env env, napi_value value);
bool IsSetValue(napi_env env, napi_value value);
bool IsCloneableTransferableValue(napi_env env, napi_value value);
bool IsTransferableValue(napi_env env, napi_value value);
bool IsBlobHandleValue(napi_env env, napi_value value);
bool IsInstanceOfValue(napi_env env, napi_value value, napi_value ctor);
bool IsFsFileHandleValue(napi_env env, napi_value value);
bool IsPlainObjectContainer(napi_env env, napi_value value);
std::string GetObjectTag(napi_env env, napi_value value);
std::string GetCtorNameForValue(napi_env env, napi_value value);
bool ValueUsesTransferArrayBuffer(napi_env env,
                                  napi_value value,
                                  napi_value target_arraybuffer,
                                  std::vector<napi_value>* seen);
napi_value GetRefValue(napi_env env, napi_ref ref);
void SetRefToValue(napi_env env, napi_ref* slot, napi_value value);

napi_value GetNamed(napi_env env, napi_value obj, const char* key) {
  napi_value out = nullptr;
  if (obj == nullptr || napi_get_named_property(env, obj, key, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool IsObjectLike(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && (type == napi_object || type == napi_function);
}

bool IsUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_undefined;
}

bool IsNullOrUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok &&
         (type == napi_undefined || type == napi_null);
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return {};
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}


void ClearPendingException(napi_env env) {
  if (env != nullptr && EdgeWorkerEnvStopRequested(env)) {
    return;
  }
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value ignored = nullptr;
    napi_get_and_clear_last_exception(env, &ignored);
  }
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void DeleteTransferredPortRefs(napi_env env,
                               std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports) {
  if (transferred_ports == nullptr) return;
  for (auto& entry : *transferred_ports) {
    DeleteRefIfPresent(env, &entry.source_port_ref);
  }
}

MessagingState& EnsureMessagingStateLocked(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<MessagingState>(
      env, kEdgeEnvironmentSlotMessagingBindingState);
}

MessagingState* FindMessagingStateLocked(napi_env env) {
  return EdgeEnvironmentGetSlotData<MessagingState>(env, kEdgeEnvironmentSlotMessagingBindingState);
}

template <typename Callback>
auto WithMessagingState(napi_env env, Callback&& callback)
    -> decltype(std::forward<Callback>(callback)(std::declval<MessagingState&>())) {
  std::lock_guard<std::mutex> lock(g_messaging_mu);
  return std::forward<Callback>(callback)(EnsureMessagingStateLocked(env));
}

template <typename Callback>
auto WithExistingMessagingState(napi_env env, Callback&& callback)
    -> decltype(std::forward<Callback>(callback)(static_cast<MessagingState*>(nullptr))) {
  std::lock_guard<std::mutex> lock(g_messaging_mu);
  return std::forward<Callback>(callback)(FindMessagingStateLocked(env));
}

template <typename Callback>
auto WithMessagingStateRefValue(napi_env env, napi_ref MessagingState::*slot, Callback&& callback)
    -> decltype(std::forward<Callback>(callback)(static_cast<napi_value>(nullptr))) {
  return WithExistingMessagingState(
      env,
      [&](MessagingState* state) {
        if (state == nullptr) {
          return std::forward<Callback>(callback)(nullptr);
        }
        return std::forward<Callback>(callback)(GetRefValue(env, state->*slot));
      });
}

void SetMessagingStateRefValue(napi_env env, napi_ref MessagingState::*slot, napi_value value) {
  if (env == nullptr) return;
  std::lock_guard<std::mutex> lock(g_messaging_mu);
  MessagingState& state = EnsureMessagingStateLocked(env);
  SetRefToValue(env, &(state.*slot), value);
}

napi_value ResolveMessagingRegisteredSymbol(napi_env env,
                                           napi_ref MessagingState::*slot,
                                           const char* name) {
  napi_value cached = WithMessagingStateRefValue(
      env,
      slot,
      [](napi_value value) { return value; });
  if (cached != nullptr && !IsUndefinedValue(env, cached)) return cached;

  napi_value global = GetGlobal(env);
  napi_value symbol_ctor = GetNamed(env, global, "Symbol");
  napi_value for_fn = GetNamed(env, symbol_ctor, "for");
  if (!IsFunction(env, for_fn)) return nullptr;

  napi_value name_value = nullptr;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &name_value) != napi_ok ||
      name_value == nullptr) {
    return nullptr;
  }

  napi_value symbol = nullptr;
  napi_value argv[1] = {name_value};
  if (napi_call_function(env, symbol_ctor, for_fn, 1, argv, &symbol) != napi_ok || symbol == nullptr) {
    ClearPendingException(env);
    return nullptr;
  }

  SetMessagingStateRefValue(env, slot, symbol);
  return symbol;
}

napi_value GetSharedHandleValue(napi_env env, uint32_t handle_id) {
  return WithExistingMessagingState(
      env,
      [&](MessagingState* state) -> napi_value {
        if (state == nullptr) return nullptr;
        auto ref_it = state->shared_handle_refs.find(handle_id);
        if (ref_it == state->shared_handle_refs.end()) return nullptr;
        return GetRefValue(env, ref_it->second);
      });
}

void ThrowTypeErrorWithCode(napi_env env, const char* code, const char* message) {
  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      code_value == nullptr ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      message_value == nullptr ||
      napi_create_type_error(env, code_value, message_value, &error_value) != napi_ok ||
      error_value == nullptr) {
    napi_throw_type_error(env, code, message);
    return;
  }
  napi_throw(env, error_value);
}

napi_value CreateDataCloneError(napi_env env, const char* message) {
  napi_value dom_exception = ResolveDOMExceptionValue(env);
  if (IsFunction(env, dom_exception)) {
    napi_value argv[2] = {nullptr, nullptr};
    napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &argv[0]);
    napi_create_string_utf8(env, "DataCloneError", NAPI_AUTO_LENGTH, &argv[1]);
    napi_value err = nullptr;
    if (napi_new_instance(env, dom_exception, 2, argv, &err) == napi_ok && err != nullptr) {
      return err;
    }
    ClearPendingException(env);
  }

  napi_value msg = nullptr;
  napi_value err = nullptr;
  napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &msg);
  napi_create_error(env, nullptr, msg, &err);
  if (err == nullptr) return nullptr;

  napi_value code = nullptr;
  napi_create_int32(env, 25, &code);
  if (code != nullptr) {
    napi_property_descriptor code_desc = {};
    code_desc.utf8name = "code";
    code_desc.value = code;
    code_desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    (void)napi_define_properties(env, err, 1, &code_desc);
  }

  napi_value name = nullptr;
  napi_create_string_utf8(env, "DataCloneError", NAPI_AUTO_LENGTH, &name);
  if (name != nullptr) {
    napi_property_descriptor name_desc = {};
    name_desc.utf8name = "name";
    name_desc.value = name;
    name_desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    (void)napi_define_properties(env, err, 1, &name_desc);
  }
  return err;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

napi_value GetInternalBindingValue(napi_env env, const char* name) {
  if (env == nullptr || name == nullptr) return nullptr;
  return edge::binding_registry::Get(env, name);
}

napi_value GetUtilPrivateSymbol(napi_env env, const char* key) {
  napi_value util_binding = GetInternalBindingValue(env, "util");
  napi_value private_symbols = GetNamed(env, util_binding, "privateSymbols");
  return GetNamed(env, private_symbols, key);
}

napi_value GetMessagingSymbol(napi_env env, const char* key) {
  napi_value symbols = GetInternalBindingValue(env, "symbols");
  return GetNamed(env, symbols, key);
}

napi_value TakePendingException(napi_env env) {
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return nullptr;

  napi_value exception = nullptr;
  if (napi_get_and_clear_last_exception(env, &exception) != napi_ok || exception == nullptr) return nullptr;
  return exception;
}

napi_value CreateErrorWithMessage(napi_env env, const char* code, const char* message) {
  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (message == nullptr ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      message_value == nullptr) {
    return nullptr;
  }
  if (code != nullptr) {
    napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value);
  }
  napi_create_error(env, code_value, message_value, &error_value);
  return error_value;
}

void SetRefToValue(napi_env env, napi_ref* slot, napi_value value) {
  if (slot == nullptr) return;
  DeleteRefIfPresent(env, slot);
  if (value == nullptr) return;
  napi_create_reference(env, value, 1, slot);
}

napi_value ResolveRegisteredSymbol(napi_env env, napi_ref* slot, const char* name) {
  napi_value cached = GetRefValue(env, slot != nullptr ? *slot : nullptr);
  if (cached != nullptr && !IsUndefinedValue(env, cached)) return cached;

  napi_value global = GetGlobal(env);
  napi_value symbol_ctor = GetNamed(env, global, "Symbol");
  napi_value for_fn = GetNamed(env, symbol_ctor, "for");
  if (!IsFunction(env, for_fn)) return nullptr;

  napi_value name_value = nullptr;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &name_value) != napi_ok ||
      name_value == nullptr) {
    return nullptr;
  }

  napi_value symbol = nullptr;
  napi_value argv[1] = {name_value};
  if (napi_call_function(env, symbol_ctor, for_fn, 1, argv, &symbol) != napi_ok || symbol == nullptr) {
    ClearPendingException(env);
    return nullptr;
  }

  SetRefToValue(env, slot, symbol);
  return symbol;
}

std::string StringifyValueForCloneError(napi_env env, napi_value value) {
  if (value == nullptr) return {};

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) {
    ClearPendingException(env);
    return {};
  }

  if (type == napi_symbol) {
    napi_value global = GetGlobal(env);
    napi_value string_ctor = GetNamed(env, global, "String");
    if (!IsFunction(env, string_ctor)) return {};
    napi_value rendered = nullptr;
    napi_value argv[1] = {value};
    if (napi_call_function(env, global, string_ctor, 1, argv, &rendered) != napi_ok || rendered == nullptr) {
      ClearPendingException(env);
      return {};
    }
    return ValueToUtf8(env, rendered);
  }

  return ValueToUtf8(env, value);
}

std::string DescribeCloneFailureValue(napi_env env, napi_value value) {
  if (value == nullptr) return {};

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) {
    ClearPendingException(env);
    return {};
  }

  if (type == napi_object) {
    if (IsPlainObjectContainer(env, value)) return {};

    bool is_array = false;
    if (napi_is_array(env, value, &is_array) == napi_ok && is_array) return {};

    if (IsArrayBufferLike(env, value) ||
        IsMapValue(env, value) ||
        IsSetValue(env, value) ||
        IsBlobHandleValue(env, value) ||
        IsTransferableValue(env, value) ||
        IsCloneableTransferableValue(env, value) ||
        UnwrapMessagePort(env, value) != nullptr) {
      return {};
    }
    return {};
  }

  if (type != napi_symbol && type != napi_function) return {};

  const std::string rendered = StringifyValueForCloneError(env, value);
  if (rendered.empty()) return {};
  return rendered + " could not be cloned.";
}

napi_value CreateArrayBufferTransferList(napi_env env, napi_value value) {
  napi_value transfer_list = GetTransferListValue(env, value);
  if (transfer_list == nullptr) return nullptr;

  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) {
    return nullptr;
  }

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length == 0) {
    return nullptr;
  }

  napi_value filtered = nullptr;
  if (napi_create_array(env, &filtered) != napi_ok || filtered == nullptr) {
    return nullptr;
  }

  uint32_t filtered_length = 0;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, transfer_list, i, &item) != napi_ok || item == nullptr) {
      continue;
    }
    bool is_arraybuffer = false;
    if (napi_is_arraybuffer(env, item, &is_arraybuffer) != napi_ok || !is_arraybuffer) {
      continue;
    }
    if (napi_set_element(env, filtered, filtered_length++, item) != napi_ok) {
      return nullptr;
    }
  }

  if (filtered_length == 0) return nullptr;
  return filtered;
}

napi_value ReadPendingCloneErrorMessage(napi_env env, napi_value fallback_value) {
  napi_value pending = TakePendingException(env);
  std::string message = DescribeCloneFailureValue(env, fallback_value);
  if (message.empty() && pending != nullptr) {
    napi_value pending_message = GetNamed(env, pending, "message");
    if (pending_message != nullptr) {
      message = ValueToUtf8(env, pending_message);
    }
  }
  if (message.empty()) {
    message = "The object could not be cloned.";
  }

  napi_value err = CreateDataCloneError(env, message.c_str());
  if (err != nullptr) {
    napi_throw(env, err);
  }
  return nullptr;
}

bool ThrowDirectCloneFailureIfDetected(napi_env env, napi_value value) {
  const std::string message = DescribeCloneFailureValue(env, value);
  if (message.empty()) return false;
  napi_value err = CreateDataCloneError(env, message.c_str());
  if (err != nullptr) napi_throw(env, err);
  return true;
}

napi_value ResolveHybridDispatchSymbol(napi_env env) {
  return ResolveMessagingRegisteredSymbol(
      env,
      &MessagingState::hybrid_dispatch_symbol_ref,
      "nodejs.internal.kHybridDispatch");
}

napi_value ResolveCurrentlyReceivingPortsSymbol(napi_env env) {
  return ResolveMessagingRegisteredSymbol(
      env,
      &MessagingState::currently_receiving_ports_symbol_ref,
      "nodejs.internal.kCurrentlyReceivingPorts");
}

napi_value GetMessagePortCtorToken(napi_env env) {
  return WithMessagingStateRefValue(
      env,
      &MessagingState::message_port_ctor_token_ref,
      [](napi_value value) { return value; });
}

bool IsInternalMessagePortCtorCall(napi_env env, size_t argc, napi_value* argv) {
  if (argc < 1 || argv == nullptr || argv[0] == nullptr) return false;
  napi_value token = GetMessagePortCtorToken(env);
  if (token == nullptr) return false;

  bool same = false;
  return napi_strict_equals(env, argv[0], token, &same) == napi_ok && same;
}

bool TryHybridDispatchMessageToPort(napi_env env,
                                    napi_value port,
                                    napi_value payload,
                                    const char* type,
                                    napi_value ports) {
  if (port == nullptr) return false;

  napi_value hybrid_dispatch_symbol = ResolveHybridDispatchSymbol(env);
  if (hybrid_dispatch_symbol == nullptr) return false;

  napi_value hook = nullptr;
  if (napi_get_property(env, port, hybrid_dispatch_symbol, &hook) != napi_ok || !IsFunction(env, hook)) {
    ClearPendingException(env);
    return false;
  }

  napi_value currently_receiving_ports_symbol = ResolveCurrentlyReceivingPortsSymbol(env);
  napi_value type_value = nullptr;
  napi_value undefined = Undefined(env);
  napi_value ignored = nullptr;
  napi_status status = napi_generic_failure;

  if (ports == nullptr) napi_create_array_with_length(env, 0, &ports);
  napi_create_string_utf8(env, type != nullptr ? type : "message", NAPI_AUTO_LENGTH, &type_value);

  if (currently_receiving_ports_symbol != nullptr) {
    napi_set_property(env, port, currently_receiving_ports_symbol, ports != nullptr ? ports : undefined);
  }

  napi_value argv[3] = {
    payload != nullptr ? payload : undefined,
    type_value != nullptr ? type_value : undefined,
    undefined,
  };
  status = EdgeMakeCallback(env, port, hook, 3, argv, &ignored);

  if (currently_receiving_ports_symbol != nullptr) {
    napi_set_property(env, port, currently_receiving_ports_symbol, undefined);
  }

  if (status == napi_ok) return true;
  ClearPendingException(env);
  return false;
}

bool IsCloneableTransferableValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return false;

  napi_value transfer_mode_symbol = GetUtilPrivateSymbol(env, "transfer_mode_private_symbol");
  napi_value clone_symbol = GetMessagingSymbol(env, "messaging_clone_symbol");
  if (clone_symbol == nullptr) return false;

  napi_value clone_method = nullptr;
  if (napi_get_property(env, value, clone_symbol, &clone_method) != napi_ok || !IsFunction(env, clone_method)) {
    return false;
  }
  if (transfer_mode_symbol == nullptr) return true;

  bool has_mode = false;
  if (napi_has_property(env, value, transfer_mode_symbol, &has_mode) != napi_ok || !has_mode) return true;

  napi_value transfer_mode = nullptr;
  if (napi_get_property(env, value, transfer_mode_symbol, &transfer_mode) != napi_ok || transfer_mode == nullptr) {
    return true;
  }

  uint32_t mode = 0;
  if (napi_get_value_uint32(env, transfer_mode, &mode) != napi_ok || (mode & 2u) == 0) return false;
  return true;
}

bool IsTransferableValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return false;

  napi_value transfer_mode_symbol = GetUtilPrivateSymbol(env, "transfer_mode_private_symbol");
  napi_value transfer_symbol = GetMessagingSymbol(env, "messaging_transfer_symbol");
  if (transfer_mode_symbol == nullptr || transfer_symbol == nullptr) return false;

  bool has_mode = false;
  if (napi_has_property(env, value, transfer_mode_symbol, &has_mode) != napi_ok || !has_mode) return false;

  napi_value transfer_mode = nullptr;
  if (napi_get_property(env, value, transfer_mode_symbol, &transfer_mode) != napi_ok || transfer_mode == nullptr) {
    return false;
  }

  uint32_t mode = 0;
  if (napi_get_value_uint32(env, transfer_mode, &mode) != napi_ok || (mode & 1u) == 0) return false;

  napi_value transfer_method = nullptr;
  return napi_get_property(env, value, transfer_symbol, &transfer_method) == napi_ok &&
         IsFunction(env, transfer_method);
}

bool IsBlobHandleValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return false;
  bool has_blob_data = false;
  return napi_has_named_property(env, value, "__edge_blob_data", &has_blob_data) == napi_ok && has_blob_data;
}

bool IsInstanceOfValue(napi_env env, napi_value value, napi_value ctor) {
  if (!IsObjectLike(env, value) || !IsFunction(env, ctor)) return false;
  bool is_instance = false;
  if (napi_instanceof(env, value, ctor, &is_instance) != napi_ok) {
    ClearPendingException(env);
    return false;
  }
  return is_instance;
}

napi_value GetBlockListBindingValue(napi_env env) {
  return GetInternalBindingValue(env, "block_list");
}

napi_value GetBlockListBindingCtor(napi_env env, const char* name) {
  return GetNamed(env, GetBlockListBindingValue(env), name);
}

bool IsSocketAddressHandleValue(napi_env env, napi_value value) {
  return IsInstanceOfValue(env, value, GetBlockListBindingCtor(env, "SocketAddress"));
}

bool IsBlockListHandleValue(napi_env env, napi_value value) {
  return IsInstanceOfValue(env, value, GetBlockListBindingCtor(env, "BlockList"));
}

napi_value CreateHandleCloneMarker(napi_env env, const char* marker_name, napi_value data) {
  if (marker_name == nullptr || data == nullptr) return nullptr;
  napi_value marker = nullptr;
  if (napi_create_object(env, &marker) != napi_ok || marker == nullptr) return nullptr;
  napi_value true_value = nullptr;
  if (napi_get_boolean(env, true, &true_value) != napi_ok || true_value == nullptr ||
      napi_set_named_property(env, marker, marker_name, true_value) != napi_ok ||
      napi_set_named_property(env, marker, "data", data) != napi_ok) {
    return nullptr;
  }
  return marker;
}

bool IsHandleCloneMarker(napi_env env,
                         napi_value value,
                         const char* marker_name,
                         napi_value* data_out) {
  if (data_out != nullptr) *data_out = nullptr;
  if (!IsObjectLike(env, value) || marker_name == nullptr) return false;

  bool has_marker = false;
  if (napi_has_named_property(env, value, marker_name, &has_marker) != napi_ok || !has_marker) {
    return false;
  }

  napi_value marker_value = GetNamed(env, value, marker_name);
  bool is_marker = false;
  if (marker_value == nullptr || napi_get_value_bool(env, marker_value, &is_marker) != napi_ok || !is_marker) {
    return false;
  }

  napi_value data = GetNamed(env, value, "data");
  if (data == nullptr) return false;
  if (data_out != nullptr) *data_out = data;
  return true;
}

napi_value CreateBlobHandleCloneMarker(napi_env env, napi_value handle) {
  napi_value blob_data = GetNamed(env, handle, "__edge_blob_data");
  if (blob_data == nullptr) return nullptr;

  return CreateHandleCloneMarker(env, "__ubiBlobHandleCloneMarker", blob_data);
}

bool IsBlobHandleCloneMarker(napi_env env, napi_value value, napi_value* data_out) {
  return IsHandleCloneMarker(env, value, "__ubiBlobHandleCloneMarker", data_out);
}

napi_value CreateSocketAddressHandleCloneMarker(napi_env env, napi_value handle) {
  napi_value detail_fn = GetNamed(env, handle, "detail");
  if (!IsFunction(env, detail_fn)) return nullptr;

  napi_value detail = nullptr;
  if (napi_create_object(env, &detail) != napi_ok || detail == nullptr) return nullptr;

  napi_value argv[1] = {detail};
  napi_value detail_out = nullptr;
  if (napi_call_function(env, handle, detail_fn, 1, argv, &detail_out) != napi_ok || detail_out == nullptr) {
    return nullptr;
  }

  return CreateHandleCloneMarker(env, "__ubiSocketAddressHandleCloneMarker", detail_out);
}

bool IsSocketAddressHandleCloneMarker(napi_env env, napi_value value, napi_value* data_out) {
  return IsHandleCloneMarker(env, value, "__ubiSocketAddressHandleCloneMarker", data_out);
}

napi_value CreateBlockListHandleCloneMarker(napi_env env, napi_value handle) {
  uint32_t id = 0;
  napi_ref handle_ref = nullptr;
  {
    std::lock_guard<std::mutex> lock(g_messaging_mu);
    MessagingState& state = EnsureMessagingStateLocked(env);
    id = state.next_shared_handle_id++;
    if (napi_create_reference(env, handle, 1, &handle_ref) != napi_ok || handle_ref == nullptr) {
      return nullptr;
    }
    state.shared_handle_refs[id] = handle_ref;
  }

  napi_value id_value = nullptr;
  if (napi_create_uint32(env, id, &id_value) != napi_ok || id_value == nullptr) {
    std::lock_guard<std::mutex> lock(g_messaging_mu);
    napi_delete_reference(env, handle_ref);
    if (MessagingState* state = FindMessagingStateLocked(env)) {
      state->shared_handle_refs.erase(id);
    }
    return nullptr;
  }

  return CreateHandleCloneMarker(env, "__ubiBlockListHandleCloneMarker", id_value);
}

bool IsBlockListHandleCloneMarker(napi_env env, napi_value value, napi_value* data_out) {
  return IsHandleCloneMarker(env, value, "__ubiBlockListHandleCloneMarker", data_out);
}

napi_value GetFsBindingValue(napi_env env) {
  return GetInternalBindingValue(env, "fs");
}

napi_value GetFsBindingCtor(napi_env env, const char* name) {
  return GetNamed(env, GetFsBindingValue(env), name);
}

bool IsFsFileHandleValue(napi_env env, napi_value value) {
  return IsInstanceOfValue(env, value, GetFsBindingCtor(env, "FileHandle"));
}

napi_value GetCryptoModuleValue(napi_env env) {
  return TryRequireModule(env, "crypto");
}

napi_value GetCryptoModuleCtor(napi_env env, const char* name) {
  return GetNamed(env, GetCryptoModuleValue(env), name);
}

bool IsCryptoKeyObjectValue(napi_env env, napi_value value) {
  return IsInstanceOfValue(env, value, GetCryptoModuleCtor(env, "KeyObject"));
}

napi_value CreateFileHandleCloneMarker(napi_env env, napi_value handle) {
  if (!IsFsFileHandleValue(env, handle)) return nullptr;

  napi_value release_fd = GetNamed(env, handle, "releaseFD");
  if (!IsFunction(env, release_fd)) return nullptr;

  napi_value fd_value = nullptr;
  if (napi_call_function(env, handle, release_fd, 0, nullptr, &fd_value) != napi_ok || fd_value == nullptr) {
    return nullptr;
  }

  int32_t fd = -1;
  if (napi_get_value_int32(env, fd_value, &fd) != napi_ok || fd < 0) {
    return nullptr;
  }

  return CreateHandleCloneMarker(env, "__ubiFileHandleCloneMarker", fd_value);
}

bool IsFileHandleCloneMarker(napi_env env, napi_value value, napi_value* data_out) {
  return IsHandleCloneMarker(env, value, "__ubiFileHandleCloneMarker", data_out);
}

napi_value CreateCryptoKeyObjectCloneMarker(napi_env env, napi_value value) {
  if (!IsCryptoKeyObjectValue(env, value)) return nullptr;
  napi_value data = EdgeCryptoCreateNativeKeyObjectCloneData(env, value);
  if (data == nullptr) return nullptr;
  return CreateHandleCloneMarker(env, "__ubiCryptoKeyObjectCloneMarker", data);
}

bool IsCryptoKeyObjectCloneMarker(napi_env env, napi_value value, napi_value* data_out) {
  return IsHandleCloneMarker(env, value, "__ubiCryptoKeyObjectCloneMarker", data_out);
}

bool IsJSTransferableCloneMarker(napi_env env, napi_value value, napi_value* data_out, napi_value* info_out) {
  if (data_out != nullptr) *data_out = nullptr;
  if (info_out != nullptr) *info_out = nullptr;
  if (!IsObjectLike(env, value)) return false;

  bool has_marker = false;
  if (napi_has_named_property(env, value, "__ubiJSTransferableCloneMarker", &has_marker) != napi_ok || !has_marker) {
    return false;
  }

  napi_value marker_value = GetNamed(env, value, "__ubiJSTransferableCloneMarker");
  bool is_marker = false;
  if (marker_value == nullptr || napi_get_value_bool(env, marker_value, &is_marker) != napi_ok || !is_marker) {
    return false;
  }

  napi_value data = GetNamed(env, value, "data");
  napi_value info = GetNamed(env, value, "deserializeInfo");
  if (data == nullptr || info == nullptr) return false;
  if (data_out != nullptr) *data_out = data;
  if (info_out != nullptr) *info_out = info;
  return true;
}

bool IsStructuredClonePassThroughValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return true;

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) return true;

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return true;

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return true;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return true;

  return false;
}

bool IsProcessEnvValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return false;

  napi_value global = GetGlobal(env);
  if (global == nullptr) return false;
  napi_value process = GetNamed(env, global, "process");
  if (!IsObjectLike(env, process)) return false;
  napi_value process_env = GetNamed(env, process, "env");
  if (!IsObjectLike(env, process_env)) return false;

  bool same = false;
  return napi_strict_equals(env, value, process_env, &same) == napi_ok && same;
}

napi_value PrepareTransferableDataForStructuredClone(napi_env env,
                                                     napi_value value,
                                                     bool allow_host_object_transfer = false);
napi_value RestoreTransferableDataAfterStructuredClone(napi_env env, napi_value value);
struct ValueTransformPair {
  napi_value source = nullptr;
  napi_value target = nullptr;
};
bool PrepareJSTransferableCloneData(
    napi_env env, napi_value value, napi_value* data_out, napi_value* deserialize_info_out);
napi_value CreateJSTransferableCloneMarker(napi_env env, napi_value value);
napi_value DeserializeJSTransferableCloneMarker(napi_env env, napi_value data, napi_value deserialize_info);
napi_value CallJSTransferableCloneFallback(napi_env env, napi_value value);
napi_value CloneRootJSTransferableValueForQueue(napi_env env, napi_value value);
bool IsPlainObjectContainer(napi_env env, napi_value value);
napi_value CreateGlobalInstance(napi_env env, const char* ctor_name);
napi_value CreateCloneTargetObject(napi_env env, napi_value source);
napi_value FindTransformedValue(napi_env env,
                                napi_value source,
                                const std::vector<ValueTransformPair>& pairs);
bool IsCloneByReferenceValue(napi_env env, napi_value value);
bool TransferListContainsValue(napi_env env, napi_value transfer_list, napi_value candidate);
napi_value TransformTransferredPortsForQueue(
    napi_env env,
    napi_value value,
    const std::vector<QueuedMessage::TransferredPortEntry>& transferred_ports,
    std::vector<ValueTransformPair>* seen_pairs);
bool AppendTransferredPortsForQueue(
    napi_env env,
    std::vector<QueuedMessage::TransferredPortEntry>* target,
    std::vector<QueuedMessage::TransferredPortEntry>* source);
bool CreateTransferredJSTransferableMarkerForQueue(
    napi_env env,
    napi_value value,
    napi_value* marker_out,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports_out);
napi_value TransformTransferredValuesForQueue(
    napi_env env,
    napi_value value,
    napi_value transfer_list,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports,
    std::vector<ValueTransformPair>* seen_pairs);
bool CollectTransferredPorts(
    napi_env env,
    napi_value transfer_list,
    std::vector<QueuedMessage::TransferredPortEntry>* out);
void DetachTransferredPorts(
    napi_env env,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports);
struct ReceivedTransferredPortState {
  std::vector<napi_value> ports;
};
napi_value RestoreTransferredPortsInValue(napi_env env,
                                          napi_value value,
                                          const QueuedMessage& message,
                                          ReceivedTransferredPortState* state,
                                          std::vector<ValueTransformPair>* seen_pairs);

napi_value CloneArrayEntriesForStructuredClone(napi_env env,
                                               napi_value array,
                                               bool allow_host_object_transfer) {
  uint32_t length = 0;
  if (napi_get_array_length(env, array, &length) != napi_ok) return nullptr;

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, length, &out) != napi_ok || out == nullptr) return nullptr;

  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, array, i, &item) != napi_ok) return nullptr;
    napi_value cloned = PrepareTransferableDataForStructuredClone(env, item, allow_host_object_transfer);
    if (cloned == nullptr) return nullptr;
    if (napi_set_element(env, out, i, cloned) != napi_ok) return nullptr;
  }
  return out;
}

napi_value CloneObjectPropertiesForStructuredClone(napi_env env,
                                                   napi_value object,
                                                   bool allow_host_object_transfer) {
  napi_value keys = nullptr;
  if (napi_get_property_names(env, object, &keys) != napi_ok || keys == nullptr) return nullptr;

  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) return nullptr;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) return nullptr;
    napi_value item = nullptr;
    if (napi_get_property(env, object, key, &item) != napi_ok) return nullptr;
    napi_value cloned = PrepareTransferableDataForStructuredClone(env, item, allow_host_object_transfer);
    if (cloned == nullptr) return nullptr;
    if (napi_set_property(env, out, key, cloned) != napi_ok) return nullptr;
  }

  return out;
}

napi_value PrepareTransferableDataForStructuredClone(napi_env env,
                                                     napi_value value,
                                                     bool allow_host_object_transfer) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsStructuredClonePassThroughValue(env, value)) {
    return value;
  }
  if (IsCloneableTransferableValue(env, value)) {
    return CreateJSTransferableCloneMarker(env, value);
  }
  if (IsBlobHandleValue(env, value)) {
    return CreateBlobHandleCloneMarker(env, value);
  }
  if (IsSocketAddressHandleValue(env, value)) {
    return CreateSocketAddressHandleCloneMarker(env, value);
  }
  if (IsBlockListHandleValue(env, value)) {
    return CreateBlockListHandleCloneMarker(env, value);
  }
  if (allow_host_object_transfer && IsFsFileHandleValue(env, value)) {
    return CreateFileHandleCloneMarker(env, value);
  }
  if (IsCryptoKeyObjectValue(env, value)) {
    return CreateCryptoKeyObjectCloneMarker(env, value);
  }
  if (!IsObjectLike(env, value)) return value;

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    return CloneArrayEntriesForStructuredClone(env, value, allow_host_object_transfer);
  }
  if (!IsPlainObjectContainer(env, value)) {
    return value;
  }
  return CloneObjectPropertiesForStructuredClone(env, value, allow_host_object_transfer);
}

napi_value CreateBlobHandleFromCloneData(napi_env env, napi_value blob_data) {
  if (blob_data == nullptr) return nullptr;

  napi_value blob_binding = GetInternalBindingValue(env, "blob");
  napi_value create_blob = GetNamed(env, blob_binding, "createBlob");
  if (!IsFunction(env, create_blob)) return nullptr;

  size_t byte_length = 0;
  void* raw = nullptr;
  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, blob_data, &is_arraybuffer) != napi_ok || !is_arraybuffer ||
      napi_get_arraybuffer_info(env, blob_data, &raw, &byte_length) != napi_ok) {
    return nullptr;
  }

  napi_value sources = nullptr;
  if (napi_create_array_with_length(env, 1, &sources) != napi_ok || sources == nullptr) return nullptr;
  if (napi_set_element(env, sources, 0, blob_data) != napi_ok) return nullptr;

  napi_value length_value = nullptr;
  if (napi_create_uint32(
          env,
          static_cast<uint32_t>(std::min<size_t>(byte_length, std::numeric_limits<uint32_t>::max())),
          &length_value) != napi_ok ||
      length_value == nullptr) {
    return nullptr;
  }

  napi_value argv[2] = {sources, length_value};
  napi_value handle = nullptr;
  if (napi_call_function(env, blob_binding, create_blob, 2, argv, &handle) != napi_ok || handle == nullptr) {
    return nullptr;
  }
  return handle;
}

napi_value CreateSocketAddressHandleFromCloneData(napi_env env, napi_value data) {
  napi_value ctor = GetBlockListBindingCtor(env, "SocketAddress");
  if (!IsFunction(env, ctor) || data == nullptr) return nullptr;

  napi_value argv[4] = {
      GetNamed(env, data, "address"),
      GetNamed(env, data, "port"),
      GetNamed(env, data, "family"),
      GetNamed(env, data, "flowlabel"),
  };
  if (argv[0] == nullptr || argv[1] == nullptr || argv[2] == nullptr || argv[3] == nullptr) return nullptr;

  napi_value handle = nullptr;
  if (napi_new_instance(env, ctor, 4, argv, &handle) != napi_ok || handle == nullptr) {
    return nullptr;
  }
  return handle;
}

napi_value ExtractHandleFromTransferableClone(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  napi_value clone_symbol = GetMessagingSymbol(env, "messaging_clone_symbol");
  napi_value clone_method = nullptr;
  if (clone_symbol == nullptr ||
      napi_get_property(env, value, clone_symbol, &clone_method) != napi_ok ||
      !IsFunction(env, clone_method)) {
    return nullptr;
  }

  napi_value clone_result = nullptr;
  if (napi_call_function(env, value, clone_method, 0, nullptr, &clone_result) != napi_ok || clone_result == nullptr) {
    return nullptr;
  }

  napi_value clone_data = GetNamed(env, clone_result, "data");
  return GetNamed(env, clone_data, "handle");
}

napi_value CreateBlockListHandleFromCloneData(napi_env env, napi_value rules) {
  uint32_t handle_id = 0;
  if (rules != nullptr && napi_get_value_uint32(env, rules, &handle_id) == napi_ok) {
    return GetSharedHandleValue(env, handle_id);
  }

  napi_value blocklist_module = TryRequireModule(env, "internal/blocklist");
  napi_value blocklist_ctor = GetNamed(env, blocklist_module, "BlockList");
  if (!IsFunction(env, blocklist_ctor)) return nullptr;

  napi_value blocklist = nullptr;
  if (napi_new_instance(env, blocklist_ctor, 0, nullptr, &blocklist) != napi_ok || blocklist == nullptr) {
    return nullptr;
  }

  napi_value from_json = GetNamed(env, blocklist, "fromJSON");
  if (!IsFunction(env, from_json)) return nullptr;

  napi_value argv[1] = {rules};
  napi_value ignored = nullptr;
  if (napi_call_function(env, blocklist, from_json, 1, argv, &ignored) != napi_ok) {
    return nullptr;
  }

  return ExtractHandleFromTransferableClone(env, blocklist);
}

napi_value CreateFileHandleFromCloneData(napi_env env, napi_value fd_value) {
  napi_value ctor = GetFsBindingCtor(env, "FileHandle");
  if (!IsFunction(env, ctor) || fd_value == nullptr) return nullptr;

  napi_value handle = nullptr;
  napi_value argv[1] = {fd_value};
  if (napi_new_instance(env, ctor, 1, argv, &handle) != napi_ok || handle == nullptr) {
    return nullptr;
  }
  return handle;
}

napi_value CreateCryptoKeyObjectFromCloneData(napi_env env, napi_value data) {
  return EdgeCryptoCreateKeyObjectFromCloneData(env, data);
}

napi_value RestoreTransferableDataAfterStructuredClone(napi_env env, napi_value value) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsStructuredClonePassThroughValue(env, value)) {
    return value;
  }

  napi_value transferable_data = nullptr;
  napi_value deserialize_info = nullptr;
  if (IsJSTransferableCloneMarker(env, value, &transferable_data, &deserialize_info)) {
    napi_value restored_data = RestoreTransferableDataAfterStructuredClone(env, transferable_data);
    if (restored_data == nullptr) return nullptr;
    return DeserializeJSTransferableCloneMarker(env, restored_data, deserialize_info);
  }

  napi_value blob_data = nullptr;
  if (IsBlobHandleCloneMarker(env, value, &blob_data)) {
    return CreateBlobHandleFromCloneData(env, blob_data);
  }

  napi_value socket_address_data = nullptr;
  if (IsSocketAddressHandleCloneMarker(env, value, &socket_address_data)) {
    return CreateSocketAddressHandleFromCloneData(env, socket_address_data);
  }

  napi_value block_list_data = nullptr;
  if (IsBlockListHandleCloneMarker(env, value, &block_list_data)) {
    return CreateBlockListHandleFromCloneData(env, block_list_data);
  }

  napi_value file_handle_data = nullptr;
  if (IsFileHandleCloneMarker(env, value, &file_handle_data)) {
    return CreateFileHandleFromCloneData(env, file_handle_data);
  }

  napi_value key_object_data = nullptr;
  if (IsCryptoKeyObjectCloneMarker(env, value, &key_object_data)) {
    return CreateCryptoKeyObjectFromCloneData(env, key_object_data);
  }

  if (!IsObjectLike(env, value)) return value;

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return nullptr;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok) return nullptr;
      napi_value restored = RestoreTransferableDataAfterStructuredClone(env, item);
      if (restored == nullptr || napi_set_element(env, value, i, restored) != napi_ok) return nullptr;
    }
    return value;
  }

  if (!IsPlainObjectContainer(env, value)) return value;

  napi_value keys = nullptr;
  if (napi_get_property_names(env, value, &keys) != napi_ok || keys == nullptr) return nullptr;

  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) return nullptr;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) return nullptr;
    napi_value item = nullptr;
    if (napi_get_property(env, value, key, &item) != napi_ok) return nullptr;
    napi_value restored = RestoreTransferableDataAfterStructuredClone(env, item);
    if (restored == nullptr || napi_set_property(env, value, key, restored) != napi_ok) return nullptr;
  }
  return value;
}

napi_value DeserializeJSTransferableCloneMarker(napi_env env, napi_value data, napi_value deserialize_info) {
  if (data == nullptr || deserialize_info == nullptr) return nullptr;

  napi_value deserializer_factory = WithMessagingStateRefValue(
      env,
      &MessagingState::deserializer_create_object_ref,
      [](napi_value value) { return value; });
  if (!IsFunction(env, deserializer_factory)) return nullptr;

  napi_value receiver = Undefined(env);
  napi_value factory_argv[1] = {deserialize_info};
  napi_value out = nullptr;
  if (napi_call_function(env, receiver, deserializer_factory, 1, factory_argv, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }

  napi_value deserialize_symbol = GetMessagingSymbol(env, "messaging_deserialize_symbol");
  napi_value deserialize_method = nullptr;
  if (deserialize_symbol == nullptr ||
      napi_get_property(env, out, deserialize_symbol, &deserialize_method) != napi_ok ||
      !IsFunction(env, deserialize_method)) {
    return nullptr;
  }

  napi_value deserialize_argv[1] = {data};
  napi_value ignored = nullptr;
  if (napi_call_function(env, out, deserialize_method, 1, deserialize_argv, &ignored) != napi_ok) {
    return nullptr;
  }

  return out;
}

bool PrepareJSTransferableCloneData(
    napi_env env, napi_value value, napi_value* data_out, napi_value* deserialize_info_out) {
  if (data_out != nullptr) *data_out = nullptr;
  if (deserialize_info_out != nullptr) *deserialize_info_out = nullptr;

  napi_value clone_result = nullptr;
  napi_value clone_symbol = GetMessagingSymbol(env, "messaging_clone_symbol");
  napi_value clone_method = nullptr;
  if (clone_symbol != nullptr &&
      napi_get_property(env, value, clone_symbol, &clone_method) == napi_ok &&
      IsFunction(env, clone_method)) {
    if (napi_call_function(env, value, clone_method, 0, nullptr, &clone_result) != napi_ok) {
      clone_result = nullptr;
    }
  }
  if (clone_result == nullptr) {
    ClearPendingException(env);
    clone_result = CallJSTransferableCloneFallback(env, value);
  }
  if (clone_result == nullptr || IsUndefinedValue(env, clone_result)) {
    ClearPendingException(env);
    return false;
  }

  napi_value clone_data = GetNamed(env, clone_result, "data");
  napi_value deserialize_info = GetNamed(env, clone_result, "deserializeInfo");
  if (clone_data == nullptr || deserialize_info == nullptr) return false;

  napi_value prepared_data = PrepareTransferableDataForStructuredClone(env, clone_data, false);
  if (prepared_data == nullptr) return false;

  if (data_out != nullptr) *data_out = prepared_data;
  if (deserialize_info_out != nullptr) *deserialize_info_out = deserialize_info;
  return true;
}

napi_value CallJSTransferableCloneFallback(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;

  napi_value symbols = edge::binding_registry::Get(env, "symbols");
  napi_value util = edge::binding_registry::Get(env, "util");
  if (symbols == nullptr || IsUndefined(env, symbols) ||
      util == nullptr || IsUndefined(env, util)) {
    return nullptr;
  }

  static const char kSource[] =
      "(function(value, symbols, util) {"
      "  if ((typeof value !== 'object' && typeof value !== 'function') || value === null) return undefined;"
      "  const clone = value[symbols.messaging_clone_symbol];"
      "  if (typeof clone !== 'function') return undefined;"
      "  const mode = value[util.privateSymbols.transfer_mode_private_symbol];"
      "  if (mode !== undefined && (mode & util.constants.kCloneable) === 0) return undefined;"
      "  return clone.call(value);"
      "})";

  napi_value source = nullptr;
  napi_value fn = nullptr;
  if (napi_create_string_utf8(env, kSource, NAPI_AUTO_LENGTH, &source) != napi_ok ||
      source == nullptr ||
      napi_run_script(env, source, &fn) != napi_ok ||
      !IsFunction(env, fn)) {
    ClearPendingException(env);
    return nullptr;
  }

  napi_value undefined = Undefined(env);
  napi_value argv[3] = {value, symbols, util};
  napi_value out = nullptr;
  if (napi_call_function(env, undefined, fn, 3, argv, &out) != napi_ok) {
    ClearPendingException(env);
    return nullptr;
  }
  return out;
}

bool PrepareJSTransferableTransferData(napi_env env,
                                       napi_value value,
                                       napi_value* data_out,
                                       napi_value* deserialize_info_out,
                                       napi_value* transfer_list_out) {
  if (data_out != nullptr) *data_out = nullptr;
  if (deserialize_info_out != nullptr) *deserialize_info_out = nullptr;
  if (transfer_list_out != nullptr) *transfer_list_out = nullptr;
  if (!IsTransferableValue(env, value)) return false;

  napi_value transfer_list = nullptr;
  napi_value transfer_list_symbol = GetMessagingSymbol(env, "messaging_transfer_list_symbol");
  napi_value transfer_list_method = nullptr;
  if (transfer_list_symbol != nullptr &&
      napi_get_property(env, value, transfer_list_symbol, &transfer_list_method) == napi_ok &&
      IsFunction(env, transfer_list_method)) {
    napi_value list_value = nullptr;
    if (napi_call_function(env, value, transfer_list_method, 0, nullptr, &list_value) != napi_ok) {
      return false;
    }
    transfer_list = list_value;
  }

  napi_value transfer_symbol = GetMessagingSymbol(env, "messaging_transfer_symbol");
  napi_value transfer_method = nullptr;
  if (transfer_symbol == nullptr ||
      napi_get_property(env, value, transfer_symbol, &transfer_method) != napi_ok ||
      !IsFunction(env, transfer_method)) {
    return false;
  }

  napi_value transfer_result = nullptr;
  if (napi_call_function(env, value, transfer_method, 0, nullptr, &transfer_result) != napi_ok ||
      transfer_result == nullptr) {
    return false;
  }

  napi_value transfer_data = GetNamed(env, transfer_result, "data");
  napi_value deserialize_info = GetNamed(env, transfer_result, "deserializeInfo");
  if (transfer_data == nullptr || deserialize_info == nullptr) return false;

  if (data_out != nullptr) *data_out = transfer_data;
  if (deserialize_info_out != nullptr) *deserialize_info_out = deserialize_info;
  if (transfer_list_out != nullptr) *transfer_list_out = transfer_list;
  return true;
}

napi_value CreateJSTransferableCloneMarker(napi_env env, napi_value value) {
  napi_value prepared_data = nullptr;
  napi_value deserialize_info = nullptr;
  if (!PrepareJSTransferableCloneData(env, value, &prepared_data, &deserialize_info) ||
      prepared_data == nullptr ||
      deserialize_info == nullptr) {
    return nullptr;
  }

  napi_value marker = nullptr;
  if (napi_create_object(env, &marker) != napi_ok || marker == nullptr) return nullptr;

  napi_value true_value = nullptr;
  if (napi_get_boolean(env, true, &true_value) != napi_ok || true_value == nullptr ||
      napi_set_named_property(env, marker, "__ubiJSTransferableCloneMarker", true_value) != napi_ok ||
      napi_set_named_property(env, marker, "data", prepared_data) != napi_ok ||
      napi_set_named_property(env, marker, "deserializeInfo", deserialize_info) != napi_ok) {
    return nullptr;
  }

  return marker;
}

napi_value CloneRootJSTransferableValueForQueue(napi_env env, napi_value value) {
  napi_value marker = CreateJSTransferableCloneMarker(env, value);
  if (marker == nullptr) return nullptr;

  napi_value cloned = nullptr;
  if (unofficial_napi_structured_clone(env, marker, &cloned) != napi_ok ||
      cloned == nullptr) {
    return nullptr;
  }

  return cloned;
}

bool AppendTransferredPortsForQueue(
    napi_env env,
    std::vector<QueuedMessage::TransferredPortEntry>* target,
    std::vector<QueuedMessage::TransferredPortEntry>* source) {
  if (target == nullptr || source == nullptr) return false;
  if (source->empty()) return true;
  target->reserve(target->size() + source->size());
  for (auto& entry : *source) {
    target->push_back(std::move(entry));
  }
  source->clear();
  return true;
}

bool CreateTransferredJSTransferableMarkerForQueue(
    napi_env env,
    napi_value value,
    napi_value* marker_out,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports_out) {
  if (marker_out != nullptr) *marker_out = nullptr;
  if (transferred_ports_out == nullptr) return false;

  napi_value transfer_data = nullptr;
  napi_value deserialize_info = nullptr;
  napi_value nested_transfer_list = nullptr;
  if (!PrepareJSTransferableTransferData(
          env, value, &transfer_data, &deserialize_info, &nested_transfer_list) ||
      transfer_data == nullptr ||
      deserialize_info == nullptr) {
    return false;
  }

  std::vector<QueuedMessage::TransferredPortEntry> nested_ports;
  if (!CollectTransferredPorts(env, nested_transfer_list, &nested_ports)) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  std::vector<ValueTransformPair> nested_seen_pairs;
  napi_value transformed_data =
      TransformTransferredPortsForQueue(env, transfer_data, nested_ports, &nested_seen_pairs);
  if (transformed_data == nullptr) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  napi_value prepared_data = PrepareTransferableDataForStructuredClone(env, transformed_data, true);
  if (prepared_data == nullptr) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  napi_value marker = nullptr;
  if (napi_create_object(env, &marker) != napi_ok || marker == nullptr) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  napi_value true_value = nullptr;
  if (napi_get_boolean(env, true, &true_value) != napi_ok || true_value == nullptr ||
      napi_set_named_property(env, marker, "__ubiJSTransferableCloneMarker", true_value) != napi_ok ||
      napi_set_named_property(env, marker, "data", prepared_data) != napi_ok ||
      napi_set_named_property(env, marker, "deserializeInfo", deserialize_info) != napi_ok) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  if (!AppendTransferredPortsForQueue(env, transferred_ports_out, &nested_ports)) {
    DeleteTransferredPortRefs(env, &nested_ports);
    return false;
  }

  napi_value cloned_marker = nullptr;
  if (unofficial_napi_structured_clone(env, marker, &cloned_marker) != napi_ok ||
      cloned_marker == nullptr) {
    return false;
  }

  if (marker_out != nullptr) *marker_out = cloned_marker;
  return true;
}

napi_value TransformTransferredValuesForQueue(
    napi_env env,
    napi_value value,
    napi_value transfer_list,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports,
    std::vector<ValueTransformPair>* seen_pairs) {
  if (transfer_list != nullptr &&
      TransferListContainsValue(env, transfer_list, value) &&
      IsTransferableValue(env, value)) {
    napi_value marker = nullptr;
    if (CreateTransferredJSTransferableMarkerForQueue(env, value, &marker, transferred_ports) && marker != nullptr) {
      return marker;
    }
    return nullptr;
  }

  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsCloneByReferenceValue(env, value)) {
    return value;
  }
  if (IsCloneableTransferableValue(env, value) || IsTransferableValue(env, value)) {
    return value;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return value;
  }

  if (seen_pairs != nullptr) {
    napi_value existing = FindTransformedValue(env, value, *seen_pairs);
    if (existing != nullptr) return existing;
  }

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return value;
    napi_value out = nullptr;
    if (napi_create_array_with_length(env, length, &out) != napi_ok || out == nullptr) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) continue;
      napi_value transformed =
          TransformTransferredValuesForQueue(env, item, transfer_list, transferred_ports, seen_pairs);
      if (transformed == nullptr) return nullptr;
      napi_set_element(env, out, i, transformed);
    }
    return out;
  }

  if (IsMapValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value out = CreateGlobalInstance(env, "Map");
    napi_value set_fn = GetNamed(env, out, "set");
    if (!IsFunction(env, from_fn) || out == nullptr || !IsFunction(env, set_fn)) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return out;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return out;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value pair = nullptr;
      if (napi_get_element(env, entries, i, &pair) != napi_ok || pair == nullptr) continue;
      napi_value key = nullptr;
      napi_value map_value = nullptr;
      if (napi_get_element(env, pair, 0, &key) != napi_ok || key == nullptr) continue;
      if (napi_get_element(env, pair, 1, &map_value) != napi_ok) map_value = Undefined(env);
      napi_value transformed_key =
          TransformTransferredValuesForQueue(env, key, transfer_list, transferred_ports, seen_pairs);
      napi_value transformed_value =
          TransformTransferredValuesForQueue(env, map_value, transfer_list, transferred_ports, seen_pairs);
      if (transformed_key == nullptr || transformed_value == nullptr) return nullptr;
      napi_value set_argv[2] = {transformed_key, transformed_value};
      napi_value ignored = nullptr;
      if (napi_call_function(env, out, set_fn, 2, set_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return out;
  }

  if (IsSetValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value out = CreateGlobalInstance(env, "Set");
    napi_value add_fn = GetNamed(env, out, "add");
    if (!IsFunction(env, from_fn) || out == nullptr || !IsFunction(env, add_fn)) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return out;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return out;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, entries, i, &item) != napi_ok || item == nullptr) continue;
      napi_value transformed =
          TransformTransferredValuesForQueue(env, item, transfer_list, transferred_ports, seen_pairs);
      if (transformed == nullptr) return nullptr;
      napi_value add_argv[1] = {transformed};
      napi_value ignored = nullptr;
      if (napi_call_function(env, out, add_fn, 1, add_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return out;
  }

  if (!IsPlainObjectContainer(env, value)) {
    return value;
  }

  napi_value out = CreateCloneTargetObject(env, value);
  if (out == nullptr) return value;
  if (seen_pairs != nullptr) seen_pairs->push_back({value, out});

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, value, napi_key_all_properties, &keys) != napi_ok ||
      keys == nullptr) {
    return out;
  }
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return out;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value child = nullptr;
    if (napi_get_property(env, value, key, &child) != napi_ok || child == nullptr) continue;
    napi_value transformed =
        TransformTransferredValuesForQueue(env, child, transfer_list, transferred_ports, seen_pairs);
    if (transformed == nullptr) return nullptr;
    napi_set_property(env, out, key, transformed);
  }
  return out;
}

bool TransferRootJSTransferableValueForQueue(
    napi_env env,
    napi_value value,
    napi_value* cloned_out,
    std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports_out) {
  if (cloned_out != nullptr) *cloned_out = nullptr;
  if (transferred_ports_out != nullptr) transferred_ports_out->clear();

  napi_value transfer_data = nullptr;
  napi_value deserialize_info = nullptr;
  napi_value nested_transfer_list = nullptr;
  if (!PrepareJSTransferableTransferData(
          env, value, &transfer_data, &deserialize_info, &nested_transfer_list) ||
      transfer_data == nullptr ||
      deserialize_info == nullptr) {
    return false;
  }

  if (transferred_ports_out != nullptr &&
      !CollectTransferredPorts(env, nested_transfer_list, transferred_ports_out)) {
    return false;
  }

  std::vector<ValueTransformPair> seen_pairs;
  const std::vector<QueuedMessage::TransferredPortEntry> empty_transferred_ports;
  const auto& transferred_ports =
      transferred_ports_out != nullptr ? *transferred_ports_out : empty_transferred_ports;
  napi_value transformed_data =
      TransformTransferredPortsForQueue(env, transfer_data, transferred_ports, &seen_pairs);
  if (transformed_data == nullptr) return false;

  napi_value prepared_data = PrepareTransferableDataForStructuredClone(env, transformed_data, true);
  if (prepared_data == nullptr) return false;

  napi_value marker = nullptr;
  if (napi_create_object(env, &marker) != napi_ok || marker == nullptr) return false;
  napi_value true_value = nullptr;
  if (napi_get_boolean(env, true, &true_value) != napi_ok || true_value == nullptr ||
      napi_set_named_property(env, marker, "__ubiJSTransferableCloneMarker", true_value) != napi_ok ||
      napi_set_named_property(env, marker, "data", prepared_data) != napi_ok ||
      napi_set_named_property(env, marker, "deserializeInfo", deserialize_info) != napi_ok) {
    return false;
  }

  napi_value cloned = nullptr;
  if (unofficial_napi_structured_clone(env, marker, &cloned) != napi_ok ||
      cloned == nullptr) {
    return false;
  }

  if (cloned_out != nullptr) *cloned_out = cloned;
  return true;
}

napi_value StructuredCloneJSTransferableValue(napi_env env, napi_value value) {
  napi_value prepared_data = nullptr;
  napi_value deserialize_info = nullptr;
  if (!PrepareJSTransferableCloneData(env, value, &prepared_data, &deserialize_info) ||
      prepared_data == nullptr ||
      deserialize_info == nullptr) {
    return nullptr;
  }

  napi_value cloned_data = nullptr;
  if (unofficial_napi_structured_clone(env, prepared_data, &cloned_data) != napi_ok ||
      cloned_data == nullptr) {
    return nullptr;
  }

  cloned_data = RestoreTransferableDataAfterStructuredClone(env, cloned_data);
  if (cloned_data == nullptr) return nullptr;

  return DeserializeJSTransferableCloneMarker(env, cloned_data, deserialize_info);
}

napi_value TryRequireModule(napi_env env, const char* module_name) {
  napi_value global = GetGlobal(env);
  if (global == nullptr) return nullptr;
  napi_value require_fn = EdgeGetRequireFunction(env);
  if (!IsFunction(env, require_fn)) {
    require_fn = GetNamed(env, global, "require");
  }
  if (!IsFunction(env, require_fn)) return nullptr;

  napi_value module_name_v = nullptr;
  if (napi_create_string_utf8(env, module_name, NAPI_AUTO_LENGTH, &module_name_v) != napi_ok ||
      module_name_v == nullptr) {
    return nullptr;
  }

  napi_value out = nullptr;
  napi_value argv[1] = {module_name_v};
  if (napi_call_function(env, global, require_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return nullptr;
  }
  return out;
}

napi_value GetUntransferableObjectPrivateSymbol(napi_env env) {
  napi_value util_binding = edge::binding_registry::Get(env, "util");
  if (util_binding == nullptr || IsUndefined(env, util_binding)) return nullptr;
  napi_value private_symbols = GetNamed(env, util_binding, "privateSymbols");
  if (private_symbols == nullptr) return nullptr;
  return GetNamed(env, private_symbols, "untransferable_object_private_symbol");
}

bool TransferListContainsMarkedUntransferable(napi_env env,
                                              napi_value transfer_list,
                                              napi_value payload = nullptr) {
  if (transfer_list == nullptr || IsUndefinedValue(env, transfer_list)) return false;

  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return false;

  napi_value marker = GetUntransferableObjectPrivateSymbol(env);
  if (marker == nullptr || IsUndefinedValue(env, marker)) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length == 0) return false;

  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, transfer_list, i, &item) != napi_ok || item == nullptr) continue;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, item, &type) != napi_ok || (type != napi_object && type != napi_function)) continue;

    bool has_marker = false;
    if (napi_has_property(env, item, marker, &has_marker) == napi_ok && has_marker) {
      napi_value marker_value = nullptr;
      if (napi_get_property(env, item, marker, &marker_value) == napi_ok && !IsUndefinedValue(env, marker_value)) {
        return true;
      }
    }
  }

  return false;
}

bool ValueUsesTransferArrayBuffer(napi_env env,
                                  napi_value value,
                                  napi_value target_arraybuffer,
                                  std::vector<napi_value>* seen) {
  if (env == nullptr || value == nullptr || target_arraybuffer == nullptr) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return false;
  }

  for (napi_value prior : *seen) {
    bool same = false;
    if (napi_strict_equals(env, prior, value, &same) == napi_ok && same) {
      return false;
    }
  }
  seen->push_back(value);

  bool same_ab = false;
  if (napi_strict_equals(env, value, target_arraybuffer, &same_ab) == napi_ok && same_ab) {
    return true;
  }

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* ignored_data = nullptr;
    size_t ignored_len = 0;
    napi_value buffer_ab = nullptr;
    if (napi_get_buffer_info(env, value, &ignored_data, &ignored_len) == napi_ok &&
        napi_get_named_property(env, value, "buffer", &buffer_ab) == napi_ok &&
        buffer_ab != nullptr) {
      bool same = false;
      if (napi_strict_equals(env, buffer_ab, target_arraybuffer, &same) == napi_ok && same) {
        return true;
      }
    }
  }

  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) == napi_ok && is_typed_array) {
    napi_typedarray_type ignored_type = napi_int8_array;
    size_t ignored_length = 0;
    void* ignored_data = nullptr;
    napi_value view_ab = nullptr;
    size_t ignored_offset = 0;
    if (napi_get_typedarray_info(
            env, value, &ignored_type, &ignored_length, &ignored_data, &view_ab, &ignored_offset) == napi_ok &&
        view_ab != nullptr) {
      bool same = false;
      if (napi_strict_equals(env, view_ab, target_arraybuffer, &same) == napi_ok && same) {
        return true;
      }
    }
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    void* ignored_data = nullptr;
    size_t ignored_length = 0;
    napi_value view_ab = nullptr;
    size_t ignored_offset = 0;
    if (napi_get_dataview_info(env, value, &ignored_length, &ignored_data, &view_ab, &ignored_offset) == napi_ok &&
        view_ab != nullptr) {
      bool same = false;
      if (napi_strict_equals(env, view_ab, target_arraybuffer, &same) == napi_ok && same) {
        return true;
      }
    }
  }

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return false;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) == napi_ok &&
          ValueUsesTransferArrayBuffer(env, item, target_arraybuffer, seen)) {
        return true;
      }
    }
    return false;
  }

  if (IsMapValue(env, value)) {
    napi_value entries = GetNamed(env, value, "entries");
    napi_value next = nullptr;
    napi_value iterator = nullptr;
    if (!IsFunction(env, entries) ||
        napi_call_function(env, value, entries, 0, nullptr, &iterator) != napi_ok ||
        iterator == nullptr ||
        (next = GetNamed(env, iterator, "next")) == nullptr ||
        !IsFunction(env, next)) {
      ClearPendingException(env);
      return false;
    }
    for (;;) {
      napi_value result = nullptr;
      if (napi_call_function(env, iterator, next, 0, nullptr, &result) != napi_ok || result == nullptr) {
        ClearPendingException(env);
        return false;
      }
      napi_value done = GetNamed(env, result, "done");
      bool is_done = false;
      if (done != nullptr && napi_get_value_bool(env, done, &is_done) == napi_ok && is_done) break;
      napi_value pair = GetNamed(env, result, "value");
      if (pair != nullptr && ValueUsesTransferArrayBuffer(env, pair, target_arraybuffer, seen)) {
        return true;
      }
    }
    return false;
  }

  if (IsSetValue(env, value)) {
    napi_value values_fn = GetNamed(env, value, "values");
    napi_value next = nullptr;
    napi_value iterator = nullptr;
    if (!IsFunction(env, values_fn) ||
        napi_call_function(env, value, values_fn, 0, nullptr, &iterator) != napi_ok ||
        iterator == nullptr ||
        (next = GetNamed(env, iterator, "next")) == nullptr ||
        !IsFunction(env, next)) {
      ClearPendingException(env);
      return false;
    }
    for (;;) {
      napi_value result = nullptr;
      if (napi_call_function(env, iterator, next, 0, nullptr, &result) != napi_ok || result == nullptr) {
        ClearPendingException(env);
        return false;
      }
      napi_value done = GetNamed(env, result, "done");
      bool is_done = false;
      if (done != nullptr && napi_get_value_bool(env, done, &is_done) == napi_ok && is_done) break;
      napi_value item = GetNamed(env, result, "value");
      if (item != nullptr && ValueUsesTransferArrayBuffer(env, item, target_arraybuffer, seen)) {
        return true;
      }
    }
    return false;
  }

  if (!IsPlainObjectContainer(env, value)) return false;

  napi_value keys = nullptr;
  if (napi_get_property_names(env, value, &keys) != napi_ok || keys == nullptr) return false;
  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    napi_value item = nullptr;
    if (napi_get_element(env, keys, i, &key) == napi_ok &&
        key != nullptr &&
        napi_get_property(env, value, key, &item) == napi_ok &&
        ValueUsesTransferArrayBuffer(env, item, target_arraybuffer, seen)) {
      return true;
    }
  }
  return false;
}

bool IsMessagePortValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_object) return false;
  napi_value ctor = WithMessagingStateRefValue(
      env,
      &MessagingState::message_port_ctor_ref,
      [](napi_value value) { return value; });
  if (!IsFunction(env, ctor)) return false;
  bool is_instance = false;
  if (napi_instanceof(env, value, ctor, &is_instance) != napi_ok || !is_instance) {
    ClearPendingException(env);
    return false;
  }
  return UnwrapMessagePort(env, value) != nullptr;
}

bool TransferListContainsValue(napi_env env, napi_value transfer_list, napi_value candidate) {
  if (transfer_list == nullptr || candidate == nullptr) return false;
  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length == 0) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, transfer_list, i, &item) != napi_ok || item == nullptr) continue;
    bool same = false;
    if (napi_strict_equals(env, item, candidate, &same) == napi_ok && same) {
      return true;
    }
  }
  return false;
}

bool TransferListContainsMessagePort(napi_env env, napi_value transfer_list, napi_value candidate) {
  return TransferListContainsValue(env, transfer_list, candidate);
}

bool TransferListContainsDuplicateMessagePort(napi_env env, napi_value transfer_list) {
  if (transfer_list == nullptr) return false;
  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length < 2) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value first = nullptr;
    if (napi_get_element(env, transfer_list, i, &first) != napi_ok || first == nullptr ||
        !IsMessagePortValue(env, first)) {
      continue;
    }
    for (uint32_t j = i + 1; j < length; ++j) {
      napi_value second = nullptr;
      if (napi_get_element(env, transfer_list, j, &second) != napi_ok || second == nullptr) continue;
      bool same = false;
      if (napi_strict_equals(env, first, second, &same) == napi_ok && same) {
        return true;
      }
    }
  }
  return false;
}

bool TransferListContainsDuplicateArrayBuffer(napi_env env, napi_value transfer_list) {
  if (transfer_list == nullptr) return false;
  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length < 2) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value first = nullptr;
    bool first_is_arraybuffer = false;
    if (napi_get_element(env, transfer_list, i, &first) != napi_ok || first == nullptr ||
        napi_is_arraybuffer(env, first, &first_is_arraybuffer) != napi_ok || !first_is_arraybuffer) {
      continue;
    }
    for (uint32_t j = i + 1; j < length; ++j) {
      napi_value second = nullptr;
      if (napi_get_element(env, transfer_list, j, &second) != napi_ok || second == nullptr) continue;
      bool same = false;
      if (napi_strict_equals(env, first, second, &same) == napi_ok && same) {
        return true;
      }
    }
  }
  return false;
}

bool ValidateTransferListMessagePorts(napi_env env, napi_value transfer_list) {
  if (transfer_list == nullptr) return true;
  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return true;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok || length == 0) return true;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, transfer_list, i, &item) != napi_ok || item == nullptr) continue;
    MessagePortWrap* wrap = UnwrapMessagePort(env, item);
    if (wrap == nullptr) continue;

    bool detached = wrap->handle_wrap.state != kEdgeHandleInitialized || !wrap->data;
    if (!detached) {
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      detached = wrap->data->closed || wrap->data->sibling.expired();
    }
    if (!detached) continue;

    napi_value err = CreateDataCloneError(env, "MessagePort in transfer list is already detached");
    if (err != nullptr) napi_throw(env, err);
    return false;
  }
  return true;
}

bool VisitedSetHas(napi_env env, napi_value visited, napi_value value);
void VisitedSetAdd(napi_env env, napi_value visited, napi_value value);

napi_value CreateVisitedSet(napi_env env) {
  napi_value global = GetGlobal(env);
  napi_value set_ctor = GetNamed(env, global, "Set");
  if (!IsFunction(env, set_ctor)) return nullptr;
  napi_value set = nullptr;
  if (napi_new_instance(env, set_ctor, 0, nullptr, &set) != napi_ok || set == nullptr) return nullptr;
  return set;
}

bool IsMarkedUncloneableValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return false;

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    return false;
  }

  napi_value transfer_mode_symbol = GetUtilPrivateSymbol(env, "transfer_mode_private_symbol");
  if (transfer_mode_symbol == nullptr) return false;

  bool has_mode = false;
  if (napi_has_property(env, value, transfer_mode_symbol, &has_mode) != napi_ok || !has_mode) {
    return false;
  }

  napi_value transfer_mode = nullptr;
  if (napi_get_property(env, value, transfer_mode_symbol, &transfer_mode) != napi_ok || transfer_mode == nullptr) {
    ClearPendingException(env);
    return false;
  }

  uint32_t mode = 0;
  if (napi_get_value_uint32(env, transfer_mode, &mode) != napi_ok) {
    ClearPendingException(env);
    return false;
  }

  return (mode & 2u) == 0;
}

bool ValueContainsMarkedUncloneable(napi_env env, napi_value value, napi_value visited) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value)) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  if (type != napi_object && type != napi_function) return false;

  if (VisitedSetHas(env, visited, value)) return false;
  VisitedSetAdd(env, visited, value);

  // Transferable-only host objects are valid as long as they are handled via
  // the transfer-list checks below, like in Node's serializer.
  if (IsTransferableValue(env, value)) return false;

  if (IsMarkedUncloneableValue(env, value)) return true;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return false;

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) return false;

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return false;

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return false;

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return false;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) continue;
      if (ValueContainsMarkedUncloneable(env, item, visited)) return true;
    }
    return false;
  }

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, value, napi_key_all_properties, &keys) != napi_ok ||
      keys == nullptr) {
    return false;
  }
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return false;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value child = nullptr;
    if (napi_get_property(env, value, key, &child) != napi_ok || child == nullptr) continue;
    if (ValueContainsMarkedUncloneable(env, child, visited)) return true;
  }
  return false;
}

bool EnsureNoMarkedUncloneableValue(napi_env env, napi_value value) {
  napi_value visited = CreateVisitedSet(env);
  if (visited == nullptr) return true;
  if (!ValueContainsMarkedUncloneable(env, value, visited)) return true;

  napi_value err = CreateDataCloneError(env, "The object could not be cloned.");
  if (err != nullptr) napi_throw(env, err);
  return false;
}

bool VisitedSetHas(napi_env env, napi_value visited, napi_value value) {
  if (visited == nullptr || value == nullptr) return false;
  napi_value has_fn = GetNamed(env, visited, "has");
  if (!IsFunction(env, has_fn)) return false;
  napi_value result = nullptr;
  napi_value argv[1] = {value};
  if (napi_call_function(env, visited, has_fn, 1, argv, &result) != napi_ok || result == nullptr) return false;
  bool has = false;
  (void)napi_get_value_bool(env, result, &has);
  return has;
}

void VisitedSetAdd(napi_env env, napi_value visited, napi_value value) {
  if (visited == nullptr || value == nullptr) return;
  napi_value add_fn = GetNamed(env, visited, "add");
  if (!IsFunction(env, add_fn)) return;
  napi_value ignored = nullptr;
  napi_value argv[1] = {value};
  (void)napi_call_function(env, visited, add_fn, 1, argv, &ignored);
}

bool ValueRequiresMessagePortTransfer(napi_env env,
                                      napi_value value,
                                      napi_value transfer_list,
                                      napi_value visited) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value)) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  if (type != napi_object && type != napi_function) return false;

  if (VisitedSetHas(env, visited, value)) return false;
  VisitedSetAdd(env, visited, value);

  if (IsMessagePortValue(env, value) || IsTransferableValue(env, value)) {
    return !TransferListContainsMessagePort(env, transfer_list, value);
  }

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return false;

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) return false;

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return false;

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return false;

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return false;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) continue;
      if (ValueRequiresMessagePortTransfer(env, item, transfer_list, visited)) return true;
    }
    return false;
  }

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, value, napi_key_all_properties, &keys) != napi_ok ||
      keys == nullptr) {
    return false;
  }
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return false;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value child = nullptr;
    if (napi_get_property(env, value, key, &child) != napi_ok || child == nullptr) continue;
    if (ValueRequiresMessagePortTransfer(env, child, transfer_list, visited)) return true;
  }
  return false;
}

bool EnsureNoMissingTransferredMessagePorts(napi_env env, napi_value value, napi_value transfer_arg) {
  napi_value transfer_list = GetTransferListValue(env, transfer_arg);
  napi_value visited = CreateVisitedSet(env);
  if (visited == nullptr) return true;
  if (!ValueRequiresMessagePortTransfer(env, value, transfer_list, visited)) return true;

  napi_value err =
      CreateDataCloneError(env, "Object that needs transfer was found in message but not listed in transferList");
  if (err != nullptr) napi_throw(env, err);
  return false;
}

napi_value GetNoMessageSymbol(napi_env env) {
  return WithMessagingStateRefValue(
      env,
      &MessagingState::no_message_symbol_ref,
      [](napi_value value) { return value; });
}

napi_value GetOnInitSymbol(napi_env env) {
  return WithMessagingStateRefValue(
      env,
      &MessagingState::oninit_symbol_ref,
      [](napi_value value) { return value; });
}

void ThrowIllegalInvocation(napi_env env) {
  napi_throw_type_error(env, nullptr, "Illegal invocation");
}

MessagePortWrap* UnwrapMessagePort(napi_env env, napi_value value) {
  MessagePortWrap* wrap = nullptr;
  if (value == nullptr) return nullptr;
  bool is_message_port = false;
  if (napi_check_object_type_tag(env, value, &kMessagePortTypeTag, &is_message_port) != napi_ok ||
      !is_message_port) {
    ClearPendingException(env);
    return nullptr;
  }
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) return nullptr;
  return wrap;
}

MessagePortWrap* UnwrapMessagePortThisOrThrow(napi_env env, napi_value value) {
  MessagePortWrap* wrap = UnwrapMessagePort(env, value);
  if (wrap == nullptr) {
    ThrowIllegalInvocation(env);
  }
  return wrap;
}

std::shared_ptr<BroadcastChannelGroup> GetOrCreateBroadcastChannelGroup(const std::string& name) {
  std::lock_guard<std::mutex> lock(g_broadcast_groups_mutex);
  auto it = g_broadcast_groups.find(name);
  if (it != g_broadcast_groups.end()) {
    if (auto existing = it->second.lock()) {
      return existing;
    }
  }

  auto group = std::make_shared<BroadcastChannelGroup>(name);
  g_broadcast_groups[name] = group;
  return group;
}

void AttachToBroadcastChannelGroup(const EdgeMessagePortDataPtr& data,
                                   const std::shared_ptr<BroadcastChannelGroup>& group) {
  if (!data || !group) return;

  {
    std::lock_guard<std::mutex> lock(data->mutex);
    data->broadcast_group = group;
    data->sibling.reset();
    data->closed = false;
    data->close_message_enqueued = false;
  }

  std::lock_guard<std::mutex> group_lock(group->mutex);
  group->members.erase(
      std::remove_if(
          group->members.begin(),
          group->members.end(),
          [&](const std::weak_ptr<EdgeMessagePortData>& entry) {
            auto member = entry.lock();
            return !member || member.get() == data.get();
          }),
      group->members.end());
  group->members.push_back(data);
}

void RemoveFromBroadcastChannelGroup(const EdgeMessagePortDataPtr& data) {
  if (!data) return;

  std::shared_ptr<BroadcastChannelGroup> group;
  {
    std::lock_guard<std::mutex> lock(data->mutex);
    group = data->broadcast_group;
    data->broadcast_group.reset();
  }
  if (!group) return;

  std::lock_guard<std::mutex> group_lock(group->mutex);
  group->members.erase(
      std::remove_if(
          group->members.begin(),
          group->members.end(),
          [&](const std::weak_ptr<EdgeMessagePortData>& entry) {
            auto member = entry.lock();
            return !member || member.get() == data.get();
          }),
      group->members.end());
}

std::vector<EdgeMessagePortDataPtr> GetBroadcastChannelTargets(const EdgeMessagePortDataPtr& source) {
  std::vector<EdgeMessagePortDataPtr> targets;
  if (!source) return targets;

  std::shared_ptr<BroadcastChannelGroup> group;
  {
    std::lock_guard<std::mutex> lock(source->mutex);
    group = source->broadcast_group;
  }
  if (!group) return targets;

  std::lock_guard<std::mutex> group_lock(group->mutex);
  group->members.erase(
      std::remove_if(
          group->members.begin(),
          group->members.end(),
          [&](const std::weak_ptr<EdgeMessagePortData>& entry) {
            auto member = entry.lock();
            if (!member) return true;
            if (member.get() == source.get()) return false;

            bool closed = false;
            {
              std::lock_guard<std::mutex> member_lock(member->mutex);
              closed = member->closed;
            }
            if (!closed) targets.push_back(member);
            return false;
          }),
      group->members.end());
  return targets;
}

EdgeMessagePortDataPtr InternalCreateMessagePortData() {
  return std::make_shared<EdgeMessagePortData>();
}

void InternalEntangleMessagePortData(const EdgeMessagePortDataPtr& first,
                                     const EdgeMessagePortDataPtr& second) {
  if (!first || !second || first.get() == second.get()) return;
  {
    std::lock_guard<std::mutex> first_lock(first->mutex);
    first->closed = false;
    first->close_message_enqueued = false;
    first->broadcast_group.reset();
    first->sibling = second;
  }
  {
    std::lock_guard<std::mutex> second_lock(second->mutex);
    second->closed = false;
    second->close_message_enqueued = false;
    second->broadcast_group.reset();
    second->sibling = first;
  }
}

bool MessagePortHasRefActive(void* data) {
  auto* wrap = static_cast<MessagePortWrap*>(data);
  if (wrap == nullptr) return false;
  if (wrap->handle_wrap.state == kEdgeHandleClosing) return wrap->closing_has_ref;
  if (wrap->handle_wrap.state != kEdgeHandleInitialized) return false;
  return EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->async));
}

napi_value MessagePortGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<MessagePortWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

void DeleteQueuedMessages(napi_env env, MessagePortWrap* wrap) {
  if (wrap == nullptr) return;
  std::deque<QueuedMessage> queued;
  {
    if (!wrap->data) return;
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    queued.swap(wrap->data->queued_messages);
    wrap->data->close_message_enqueued = false;
  }
  for (auto& entry : queued) {
    if (entry.payload_data != nullptr) {
      unofficial_napi_release_serialized_value(entry.payload_data);
      entry.payload_data = nullptr;
    }
    for (auto& port_entry : entry.transferred_ports) {
      DeleteRefIfPresent(env, &port_entry.source_port_ref);
    }
  }
}

void TriggerPortAsync(MessagePortWrap* wrap) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized ||
      uv_is_closing(reinterpret_cast<uv_handle_t*>(&wrap->async))) {
    return;
  }
  uv_async_send(&wrap->async);
}

void TriggerPortAsync(const EdgeMessagePortDataPtr& data) {
  if (!data) return;
  MessagePortWrap* wrap = nullptr;
  {
    std::lock_guard<std::mutex> lock(data->mutex);
    wrap = data->attached_port;
  }
  TriggerPortAsync(wrap);
}

napi_value GetTransferListValue(napi_env env, napi_value value) {
  if (value == nullptr || IsUndefinedValue(env, value) || IsNullOrUndefinedValue(env, value)) return nullptr;
  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) return value;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) == napi_ok && type == napi_object) {
    bool has_transfer = false;
    if (napi_has_named_property(env, value, "transfer", &has_transfer) != napi_ok || !has_transfer) {
      return nullptr;
    }
    napi_value transfer = GetNamed(env, value, "transfer");
    if (transfer != nullptr && !IsUndefinedValue(env, transfer) && !IsNullOrUndefinedValue(env, transfer)) {
      return transfer;
    }
    return nullptr;
  }
  return value;
}

bool CoerceTransferIterable(napi_env env,
                            napi_value value,
                            const char* error_message,
                            napi_value* out) {
  if (out != nullptr) *out = nullptr;
  if (value == nullptr || IsUndefinedValue(env, value)) return true;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", error_message);
    return false;
  }

  napi_value global = GetGlobal(env);
  napi_value symbol_ctor = GetNamed(env, global, "Symbol");
  napi_value iterator_symbol = GetNamed(env, symbol_ctor, "iterator");
  napi_value iterator_fn = nullptr;
  if (iterator_symbol == nullptr ||
      napi_get_property(env, value, iterator_symbol, &iterator_fn) != napi_ok ||
      !IsFunction(env, iterator_fn)) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", error_message);
    return false;
  }

  napi_value array_ctor = GetNamed(env, global, "Array");
  napi_value from_fn = GetNamed(env, array_ctor, "from");
  if (!IsFunction(env, from_fn)) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", error_message);
    return false;
  }

  napi_value argv[1] = {value};
  napi_value result = nullptr;
  if (napi_call_function(env, array_ctor, from_fn, 1, argv, &result) != napi_ok || result == nullptr) {
    ClearPendingException(env);
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", error_message);
    return false;
  }

  if (out != nullptr) *out = result;
  return true;
}

bool HasCallableIterator(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return false;
  }
  napi_value global = GetGlobal(env);
  napi_value symbol_ctor = GetNamed(env, global, "Symbol");
  napi_value iterator_symbol = GetNamed(env, symbol_ctor, "iterator");
  napi_value iterator_fn = nullptr;
  return iterator_symbol != nullptr &&
         napi_get_property(env, value, iterator_symbol, &iterator_fn) == napi_ok &&
         IsFunction(env, iterator_fn);
}

bool NormalizePostMessageTransferArg(napi_env env,
                                     napi_value arg,
                                     napi_value* normalized_arg,
                                     napi_value* transfer_list) {
  if (normalized_arg != nullptr) *normalized_arg = nullptr;
  if (transfer_list != nullptr) *transfer_list = nullptr;
  if (arg == nullptr || IsUndefinedValue(env, arg) || IsNullOrUndefinedValue(env, arg)) {
    return true;
  }

  bool is_array = false;
  if (napi_is_array(env, arg, &is_array) == napi_ok && is_array) {
    if (normalized_arg != nullptr) *normalized_arg = arg;
    if (transfer_list != nullptr) *transfer_list = arg;
    return true;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, arg, &type) == napi_ok && type == napi_object) {
    bool has_transfer = false;
    if (napi_has_named_property(env, arg, "transfer", &has_transfer) == napi_ok && has_transfer) {
      napi_value transfer = GetNamed(env, arg, "transfer");
      napi_value normalized_transfer = nullptr;
      if (transfer != nullptr && !IsUndefinedValue(env, transfer)) {
        if (!CoerceTransferIterable(
                env, transfer, "Optional options.transfer argument must be an iterable", &normalized_transfer)) {
          return false;
        }
      }
      if (normalized_transfer == nullptr) return true;
      napi_value options = nullptr;
      if (napi_create_object(env, &options) != napi_ok || options == nullptr) return false;
      napi_set_named_property(env, options, "transfer", normalized_transfer);
      if (normalized_arg != nullptr) *normalized_arg = options;
      if (transfer_list != nullptr) *transfer_list = normalized_transfer;
      return true;
    }
    if (HasCallableIterator(env, arg)) {
      napi_value normalized_transfer = nullptr;
      if (!CoerceTransferIterable(
              env, arg, "Optional transferList argument must be an iterable", &normalized_transfer)) {
        return false;
      }
      if (normalized_arg != nullptr) *normalized_arg = normalized_transfer;
      if (transfer_list != nullptr) *transfer_list = normalized_transfer;
    }
    return true;
  }

  napi_value normalized_transfer = nullptr;
  if (!CoerceTransferIterable(
          env, arg, "Optional transferList argument must be an iterable", &normalized_transfer)) {
    return false;
  }
  if (normalized_arg != nullptr) *normalized_arg = normalized_transfer;
  if (transfer_list != nullptr) *transfer_list = normalized_transfer;
  return true;
}

napi_value CloneMessageValueWithTransfers(napi_env env, napi_value value, napi_value transfer_arg) {
  napi_value normalized_transfer_arg = nullptr;
  napi_value transfer_list = nullptr;
  if (transfer_arg != nullptr &&
      !NormalizePostMessageTransferArg(env, transfer_arg, &normalized_transfer_arg, &transfer_list)) {
    return nullptr;
  }

  if (transfer_list != nullptr && TransferListContainsMarkedUntransferable(env, transfer_list, value)) {
    napi_value err = CreateDataCloneError(env, "An ArrayBuffer is marked as untransferable");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }
  if (TransferListContainsDuplicateArrayBuffer(env, transfer_list)) {
    napi_value err = CreateDataCloneError(env, "Transfer list contains duplicate ArrayBuffer");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }
  if (!ValidateTransferListMessagePorts(env, transfer_list)) {
    return nullptr;
  }
  if (TransferListContainsDuplicateMessagePort(env, transfer_list)) {
    napi_value err = CreateDataCloneError(env, "Transfer list contains duplicate MessagePort");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }

  std::vector<QueuedMessage::TransferredPortEntry> transferred_ports;
  if (!CollectTransferredPorts(env, transfer_list, &transferred_ports)) {
    return nullptr;
  }

  std::vector<ValueTransformPair> transferred_value_pairs;
  napi_value transferred_value =
      TransformTransferredValuesForQueue(env, value, transfer_list, &transferred_ports, &transferred_value_pairs);
  if (transferred_value == nullptr) {
    DeleteTransferredPortRefs(env, &transferred_ports);
    return nullptr;
  }

  std::vector<ValueTransformPair> seen_pairs;
  napi_value transformed_value =
      TransformTransferredPortsForQueue(env, transferred_value, transferred_ports, &seen_pairs);
  if (transformed_value == nullptr) {
    DeleteTransferredPortRefs(env, &transferred_ports);
    return nullptr;
  }
  transformed_value = PrepareTransferableDataForStructuredClone(env, transformed_value, false);
  if (transformed_value == nullptr) {
    DeleteTransferredPortRefs(env, &transferred_ports);
    return nullptr;
  }

  auto clone_prepared_value = [&](napi_value clone_input) -> napi_value {
    napi_value arraybuffer_transfer_list = CreateArrayBufferTransferList(env, normalized_transfer_arg);
    napi_value cloned = nullptr;
    const napi_status clone_status =
        unofficial_napi_structured_clone_with_transfer(
            env, clone_input, arraybuffer_transfer_list, &cloned);
    if (clone_status != napi_ok || cloned == nullptr) {
      bool has_pending = false;
      if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
        return ReadPendingCloneErrorMessage(env, clone_input);
      }
      std::string message = DescribeCloneFailureValue(env, clone_input);
      napi_value err = CreateDataCloneError(
          env, message.empty() ? "The object could not be cloned." : message.c_str());
      if (err != nullptr) napi_throw(env, err);
      return nullptr;
    }
    return cloned;
  };

  napi_value cloned = clone_prepared_value(transformed_value);
  if (cloned == nullptr) {
    DeleteTransferredPortRefs(env, &transferred_ports);
    return nullptr;
  }

  QueuedMessage message;
  message.transferred_ports = std::move(transferred_ports);
  DetachTransferredPorts(env, &message.transferred_ports);

  if (!message.transferred_ports.empty()) {
    ReceivedTransferredPortState received_ports;
    std::vector<ValueTransformPair> restore_pairs;
    cloned = RestoreTransferredPortsInValue(env, cloned, message, &received_ports, &restore_pairs);
    if (cloned == nullptr) {
      DeleteTransferredPortRefs(env, &message.transferred_ports);
      return nullptr;
    }
  }

  cloned = RestoreTransferableDataAfterStructuredClone(env, cloned);
  DeleteTransferredPortRefs(env, &message.transferred_ports);
  return cloned;
}

napi_value CloneMessageValue(napi_env env, napi_value value, napi_value transfer_arg) {
  napi_value clone_input = value;
  if (IsProcessEnvValue(env, clone_input)) {
    clone_input = CloneObjectPropertiesForStructuredClone(env, clone_input, false);
    if (clone_input == nullptr) return nullptr;
  }
  clone_input = PrepareTransferableDataForStructuredClone(env, clone_input, false);
  if (clone_input == nullptr) return nullptr;

  auto clone_prepared_value = [&](napi_value prepared_value) -> napi_value {
    napi_value cloned = nullptr;
    const napi_status clone_status =
        unofficial_napi_structured_clone(env, prepared_value, &cloned);
    if (clone_status != napi_ok || cloned == nullptr) {
      bool has_pending = false;
      if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
        return ReadPendingCloneErrorMessage(env, prepared_value);
      }
      std::string message = DescribeCloneFailureValue(env, prepared_value);
      napi_value err = CreateDataCloneError(
          env, message.empty() ? "The object could not be cloned." : message.c_str());
      if (err != nullptr) napi_throw(env, err);
      return nullptr;
    }
    return cloned;
  };

  napi_value cloned = clone_prepared_value(clone_input);
  if (cloned == nullptr) return nullptr;
  return RestoreTransferableDataAfterStructuredClone(env, cloned);
}

void EmitProcessWarning(napi_env env, const char* message) {
  if (env == nullptr || message == nullptr) return;
  napi_value global = GetGlobal(env);
  napi_value process = GetNamed(env, global, "process");
  napi_value emit_warning = GetNamed(env, process, "emitWarning");
  if (!IsFunction(env, emit_warning)) return;

  napi_value warning = nullptr;
  if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &warning) != napi_ok || warning == nullptr) return;

  napi_value ignored = nullptr;
  if (napi_call_function(env, process, emit_warning, 1, &warning, &ignored) != napi_ok) {
    ClearPendingException(env);
  }
}

void ThrowClosedMessagePortError(napi_env env) {
  napi_throw_error(env, "ERR_CLOSED_MESSAGE_PORT", "Cannot send data on closed MessagePort");
}

void OnMessagePortClosed(uv_handle_t* handle);

bool SerializeMessageValueForQueue(napi_env env, napi_value payload, void** payload_out) {
  if (payload_out == nullptr) return false;
  *payload_out = nullptr;
  if (payload == nullptr) return true;

  napi_value queue_payload = PrepareTransferableDataForStructuredClone(env, payload, false);
  if (queue_payload == nullptr) return false;
  return unofficial_napi_serialize_value(env, queue_payload, payload_out) == napi_ok &&
         *payload_out != nullptr;
}

bool IsCloneByReferenceValue(napi_env env, napi_value value) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value)) return true;
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return true;
  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) return true;
  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return true;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return true;
  return false;
}

std::string GetObjectTag(napi_env env, napi_value value) {
  napi_value global = GetGlobal(env);
  napi_value object_ctor = GetNamed(env, global, "Object");
  napi_value prototype = GetNamed(env, object_ctor, "prototype");
  napi_value to_string = GetNamed(env, prototype, "toString");
  if (!IsFunction(env, to_string)) return {};
  napi_value out = nullptr;
  if (napi_call_function(env, value, to_string, 0, nullptr, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return {};
  }
  return ValueToUtf8(env, out);
}

std::string GetCtorNameForValue(napi_env env, napi_value value) {
  if (!IsObjectLike(env, value)) return {};
  napi_value ctor = GetNamed(env, value, "constructor");
  if (!IsFunction(env, ctor)) return {};
  napi_value name = GetNamed(env, ctor, "name");
  if (name == nullptr) return {};
  return ValueToUtf8(env, name);
}

bool IsInstanceOfGlobalCtor(napi_env env, napi_value value, const char* ctor_name) {
  if (value == nullptr || ctor_name == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return false;
  }
  napi_value global = GetGlobal(env);
  napi_value ctor = GetNamed(env, global, ctor_name);
  if (!IsFunction(env, ctor)) return false;
  bool is_instance = false;
  return napi_instanceof(env, value, ctor, &is_instance) == napi_ok && is_instance;
}

bool IsMapValue(napi_env env, napi_value value) {
  const std::string tag = GetObjectTag(env, value);
  return tag == "[object Map]" || IsInstanceOfGlobalCtor(env, value, "Map");
}

bool IsSetValue(napi_env env, napi_value value) {
  const std::string tag = GetObjectTag(env, value);
  return tag == "[object Set]" || IsInstanceOfGlobalCtor(env, value, "Set");
}

bool IsArrayBufferLike(napi_env env, napi_value value) {
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return true;
  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) return true;
  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) return true;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return true;
  return false;
}

bool IsPlainObjectContainer(napi_env env, napi_value value) {
  if (value == nullptr) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_object) {
    return false;
  }

  napi_value prototype = nullptr;
  if (napi_get_prototype(env, value, &prototype) != napi_ok) {
    ClearPendingException(env);
    return false;
  }
  if (prototype == nullptr || IsNullOrUndefinedValue(env, prototype)) {
    return true;
  }

  napi_value global = GetGlobal(env);
  napi_value object_ctor = GetNamed(env, global, "Object");
  napi_value object_prototype = GetNamed(env, object_ctor, "prototype");
  if (object_prototype == nullptr) return false;

  bool same = false;
  return napi_strict_equals(env, prototype, object_prototype, &same) == napi_ok && same;
}

napi_value CreateGlobalInstance(napi_env env, const char* ctor_name) {
  napi_value global = GetGlobal(env);
  napi_value ctor = GetNamed(env, global, ctor_name);
  if (!IsFunction(env, ctor)) return nullptr;
  napi_value out = nullptr;
  if (napi_new_instance(env, ctor, 0, nullptr, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return nullptr;
  }
  return out;
}

bool FindTransferredPortIndex(napi_env env,
                              napi_value value,
                              const std::vector<QueuedMessage::TransferredPortEntry>& transferred_ports,
                              uint32_t* index_out) {
  if (index_out != nullptr) *index_out = 0;
  for (uint32_t i = 0; i < transferred_ports.size(); ++i) {
    napi_value source_port = GetRefValue(env, transferred_ports[i].source_port_ref);
    if (source_port == nullptr) continue;
    bool same = false;
    if (napi_strict_equals(env, source_port, value, &same) == napi_ok && same) {
      if (index_out != nullptr) *index_out = i;
      return true;
    }
  }
  return false;
}

napi_value CreateTransferPlaceholder(napi_env env, uint32_t index) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;
  SetInt32(env, out, "__ubiMessagePortTransferIndex", static_cast<int32_t>(index));
  return out;
}

bool ReadTransferPlaceholderIndex(napi_env env, napi_value value, uint32_t* index_out) {
  if (index_out != nullptr) *index_out = 0;
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_object) return false;
  bool has_index = false;
  if (napi_has_named_property(env, value, "__ubiMessagePortTransferIndex", &has_index) != napi_ok || !has_index) {
    return false;
  }
  napi_value index_value = GetNamed(env, value, "__ubiMessagePortTransferIndex");
  if (index_value == nullptr) return false;
  int32_t index = 0;
  if (napi_get_value_int32(env, index_value, &index) != napi_ok || index < 0) return false;
  if (index_out != nullptr) *index_out = static_cast<uint32_t>(index);
  return true;
}

napi_value CreateCloneTargetObject(napi_env env, napi_value source) {
  napi_value prototype = nullptr;
  if (napi_get_prototype(env, source, &prototype) != napi_ok || prototype == nullptr) {
    prototype = nullptr;
  }

  napi_value global = GetGlobal(env);
  napi_value object_ctor = GetNamed(env, global, "Object");
  napi_value create_fn = GetNamed(env, object_ctor, "create");
  if (!IsFunction(env, create_fn) || prototype == nullptr) {
    napi_value out = nullptr;
    napi_create_object(env, &out);
    return out;
  }

  napi_value out = nullptr;
  napi_value argv[1] = {prototype};
  if (napi_call_function(env, object_ctor, create_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    napi_create_object(env, &out);
  }
  return out;
}

napi_value FindTransformedValue(napi_env env,
                                napi_value source,
                                const std::vector<ValueTransformPair>& pairs) {
  for (const auto& pair : pairs) {
    bool same = false;
    if (pair.source != nullptr && napi_strict_equals(env, pair.source, source, &same) == napi_ok && same) {
      return pair.target;
    }
  }
  return nullptr;
}

napi_value TransformTransferredPortsForQueue(
    napi_env env,
    napi_value value,
    const std::vector<QueuedMessage::TransferredPortEntry>& transferred_ports,
    std::vector<ValueTransformPair>* seen_pairs) {
  uint32_t transfer_index = 0;
  if (FindTransferredPortIndex(env, value, transferred_ports, &transfer_index)) {
    return CreateTransferPlaceholder(env, transfer_index);
  }

  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsCloneByReferenceValue(env, value)) {
    return value;
  }
  if (IsCloneableTransferableValue(env, value)) {
    return value;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return value;
  }

  if (seen_pairs != nullptr) {
    napi_value existing = FindTransformedValue(env, value, *seen_pairs);
    if (existing != nullptr) return existing;
  }

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return value;
    napi_value out = nullptr;
    if (napi_create_array_with_length(env, length, &out) != napi_ok || out == nullptr) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) continue;
      napi_value transformed =
          TransformTransferredPortsForQueue(env, item, transferred_ports, seen_pairs);
      napi_set_element(env, out, i, transformed != nullptr ? transformed : item);
    }
    return out;
  }

  if (IsMapValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value out = CreateGlobalInstance(env, "Map");
    napi_value set_fn = GetNamed(env, out, "set");
    if (!IsFunction(env, from_fn) || out == nullptr || !IsFunction(env, set_fn)) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return out;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return out;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value pair = nullptr;
      if (napi_get_element(env, entries, i, &pair) != napi_ok || pair == nullptr) continue;
      napi_value key = nullptr;
      napi_value map_value = nullptr;
      if (napi_get_element(env, pair, 0, &key) != napi_ok || key == nullptr) continue;
      if (napi_get_element(env, pair, 1, &map_value) != napi_ok) map_value = Undefined(env);
      napi_value transformed_key =
          TransformTransferredPortsForQueue(env, key, transferred_ports, seen_pairs);
      napi_value transformed_value =
          TransformTransferredPortsForQueue(env, map_value, transferred_ports, seen_pairs);
      napi_value set_argv[2] = {transformed_key != nullptr ? transformed_key : key,
                                transformed_value != nullptr ? transformed_value : map_value};
      napi_value ignored = nullptr;
      if (napi_call_function(env, out, set_fn, 2, set_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return out;
  }

  if (IsSetValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value out = CreateGlobalInstance(env, "Set");
    napi_value add_fn = GetNamed(env, out, "add");
    if (!IsFunction(env, from_fn) || out == nullptr || !IsFunction(env, add_fn)) return value;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return out;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return out;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, entries, i, &item) != napi_ok || item == nullptr) continue;
      napi_value transformed =
          TransformTransferredPortsForQueue(env, item, transferred_ports, seen_pairs);
      napi_value add_argv[1] = {transformed != nullptr ? transformed : item};
      napi_value ignored = nullptr;
      if (napi_call_function(env, out, add_fn, 1, add_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return out;
  }

  if (!IsPlainObjectContainer(env, value)) {
    return value;
  }

  napi_value out = CreateCloneTargetObject(env, value);
  if (out == nullptr) return value;
  if (seen_pairs != nullptr) seen_pairs->push_back({value, out});

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, value, napi_key_all_properties, &keys) != napi_ok ||
      keys == nullptr) {
    return out;
  }
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return out;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value child = nullptr;
    if (napi_get_property(env, value, key, &child) != napi_ok || child == nullptr) continue;
    napi_value transformed =
        TransformTransferredPortsForQueue(env, child, transferred_ports, seen_pairs);
    napi_set_property(env, out, key, transformed != nullptr ? transformed : child);
  }
  return out;
}

bool CollectTransferredPorts(
    napi_env env,
    napi_value transfer_list,
    std::vector<QueuedMessage::TransferredPortEntry>* out) {
  if (out == nullptr || transfer_list == nullptr) return true;
  bool is_array = false;
  if (napi_is_array(env, transfer_list, &is_array) != napi_ok || !is_array) return true;

  uint32_t length = 0;
  if (napi_get_array_length(env, transfer_list, &length) != napi_ok) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, transfer_list, i, &item) != napi_ok || item == nullptr) continue;
    MessagePortWrap* wrap = UnwrapMessagePort(env, item);
    if (wrap == nullptr || wrap->data == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) continue;
    QueuedMessage::TransferredPortEntry entry;
    if (napi_create_reference(env, item, 1, &entry.source_port_ref) != napi_ok) return false;
    entry.data = wrap->data;
    out->push_back(std::move(entry));
  }
  return true;
}

bool DetachTransferredPort(napi_env env, MessagePortWrap* wrap) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized || !wrap->data) return false;
  RemoveFromBroadcastChannelGroup(wrap->data);
  {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    if (wrap->data->attached_port == wrap) {
      wrap->data->attached_port = nullptr;
    }
  }
  wrap->closing_has_ref =
      EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->async));
  wrap->data.reset();
  wrap->handle_wrap.state = kEdgeHandleClosing;
  uv_close(reinterpret_cast<uv_handle_t*>(&wrap->async), OnMessagePortClosed);
  return true;
}

void DetachTransferredPorts(napi_env env,
                            std::vector<QueuedMessage::TransferredPortEntry>* transferred_ports) {
  if (transferred_ports == nullptr) return;
  for (auto& entry : *transferred_ports) {
    napi_value source_port = GetRefValue(env, entry.source_port_ref);
    MessagePortWrap* transferred_wrap = UnwrapMessagePort(env, source_port);
    if (transferred_wrap != nullptr) {
      DetachTransferredPort(env, transferred_wrap);
    }
    DeleteRefIfPresent(env, &entry.source_port_ref);
  }
}

napi_value GetOrCreateReceivedTransferredPort(napi_env env,
                                              const QueuedMessage& message,
                                              ReceivedTransferredPortState* state,
                                              uint32_t index) {
  if (state == nullptr || index >= message.transferred_ports.size()) return nullptr;
  if (state->ports.size() < message.transferred_ports.size()) {
    state->ports.resize(message.transferred_ports.size(), nullptr);
  }
  if (state->ports[index] != nullptr) return state->ports[index];
  napi_value port = EdgeCreateMessagePortForData(env, message.transferred_ports[index].data);
  state->ports[index] = port;
  return port;
}

napi_value RestoreTransferredPortsInValue(napi_env env,
                                          napi_value value,
                                          const QueuedMessage& message,
                                          ReceivedTransferredPortState* state,
                                          std::vector<ValueTransformPair>* seen_pairs) {
  uint32_t placeholder_index = 0;
  if (ReadTransferPlaceholderIndex(env, value, &placeholder_index)) {
    return GetOrCreateReceivedTransferredPort(env, message, state, placeholder_index);
  }

  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsCloneByReferenceValue(env, value)) {
    return value;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return value;
  }

  if (seen_pairs != nullptr) {
    napi_value existing = FindTransformedValue(env, value, *seen_pairs);
    if (existing != nullptr) return existing;
    seen_pairs->push_back({value, value});
  }

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return value;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) continue;
      napi_value restored = RestoreTransferredPortsInValue(env, item, message, state, seen_pairs);
      if (restored != nullptr) napi_set_element(env, value, i, restored);
    }
    return value;
  }

  if (IsMapValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value clear_fn = GetNamed(env, value, "clear");
    napi_value set_fn = GetNamed(env, value, "set");
    if (!IsFunction(env, from_fn) || !IsFunction(env, clear_fn) || !IsFunction(env, set_fn)) {
      return value;
    }
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return value;
    }
    napi_value ignored = nullptr;
    if (napi_call_function(env, value, clear_fn, 0, nullptr, &ignored) != napi_ok) {
      ClearPendingException(env);
      return value;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return value;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value pair = nullptr;
      if (napi_get_element(env, entries, i, &pair) != napi_ok || pair == nullptr) continue;
      napi_value key = nullptr;
      napi_value map_value = nullptr;
      if (napi_get_element(env, pair, 0, &key) != napi_ok || key == nullptr) continue;
      if (napi_get_element(env, pair, 1, &map_value) != napi_ok) map_value = Undefined(env);
      napi_value restored_key = RestoreTransferredPortsInValue(env, key, message, state, seen_pairs);
      napi_value restored_value = RestoreTransferredPortsInValue(env, map_value, message, state, seen_pairs);
      napi_value set_argv[2] = {restored_key != nullptr ? restored_key : key,
                                restored_value != nullptr ? restored_value : map_value};
      if (napi_call_function(env, value, set_fn, 2, set_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return value;
  }

  if (IsSetValue(env, value)) {
    napi_value entries = nullptr;
    napi_value global = GetGlobal(env);
    napi_value array_ctor = GetNamed(env, global, "Array");
    napi_value from_fn = GetNamed(env, array_ctor, "from");
    napi_value clear_fn = GetNamed(env, value, "clear");
    napi_value add_fn = GetNamed(env, value, "add");
    if (!IsFunction(env, from_fn) || !IsFunction(env, clear_fn) || !IsFunction(env, add_fn)) {
      return value;
    }
    napi_value argv[1] = {value};
    if (napi_call_function(env, array_ctor, from_fn, 1, argv, &entries) != napi_ok || entries == nullptr) {
      ClearPendingException(env);
      return value;
    }
    napi_value ignored = nullptr;
    if (napi_call_function(env, value, clear_fn, 0, nullptr, &ignored) != napi_ok) {
      ClearPendingException(env);
      return value;
    }
    uint32_t length = 0;
    if (napi_get_array_length(env, entries, &length) != napi_ok) return value;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, entries, i, &item) != napi_ok || item == nullptr) continue;
      napi_value restored = RestoreTransferredPortsInValue(env, item, message, state, seen_pairs);
      napi_value add_argv[1] = {restored != nullptr ? restored : item};
      if (napi_call_function(env, value, add_fn, 1, add_argv, &ignored) != napi_ok) {
        ClearPendingException(env);
      }
    }
    return value;
  }

  if (!IsPlainObjectContainer(env, value)) {
    return value;
  }

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, value, napi_key_all_properties, &keys) != napi_ok ||
      keys == nullptr) {
    return value;
  }
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return value;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value child = nullptr;
    if (napi_get_property(env, value, key, &child) != napi_ok || child == nullptr) continue;
    napi_value restored = RestoreTransferredPortsInValue(env, child, message, state, seen_pairs);
    if (restored != nullptr) napi_set_property(env, value, key, restored);
  }
  return value;
}

napi_value BuildTransferredPortsArray(napi_env env,
                                      const QueuedMessage& message,
                                      ReceivedTransferredPortState* state) {
  if (message.transferred_ports.empty()) return nullptr;

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, message.transferred_ports.size(), &out) != napi_ok ||
      out == nullptr) {
    return nullptr;
  }
  for (uint32_t i = 0; i < message.transferred_ports.size(); ++i) {
    napi_value port = GetOrCreateReceivedTransferredPort(env, message, state, i);
    napi_set_element(env, out, i, port != nullptr ? port : Undefined(env));
  }
  return out;
}

void EnqueueQueuedMessage(napi_env env,
                          const EdgeMessagePortDataPtr& target,
                          QueuedMessage queued) {
  if (!target) return;
  {
    std::lock_guard<std::mutex> lock(target->mutex);
    if (queued.is_close) {
      if (target->closed || target->close_message_enqueued) {
        if (queued.payload_data != nullptr) {
          unofficial_napi_release_serialized_value(queued.payload_data);
          queued.payload_data = nullptr;
        }
        return;
      }
      target->close_message_enqueued = true;
    }
    if (target->closed) {
      if (queued.payload_data != nullptr) {
        unofficial_napi_release_serialized_value(queued.payload_data);
        queued.payload_data = nullptr;
      }
      for (auto& entry : queued.transferred_ports) {
        DeleteRefIfPresent(env, &entry.source_port_ref);
      }
      return;
    }
    target->queued_messages.push_back(std::move(queued));
  }
  TriggerPortAsync(target);
}

void EnqueueMessageToPort(napi_env env,
                          const EdgeMessagePortDataPtr& target,
                          napi_value payload,
                          bool is_close,
                          MessagePortWrap* close_source_wrap = nullptr,
                          std::vector<QueuedMessage::TransferredPortEntry> transferred_ports = {}) {
  if (!target) return;
  QueuedMessage queued;
  queued.is_close = is_close;
  queued.close_source = close_source_wrap;
  queued.transferred_ports = std::move(transferred_ports);
  if (!is_close && payload != nullptr) {
    if (!SerializeMessageValueForQueue(env, payload, &queued.payload_data)) {
      ClearPendingException(env);
      for (auto& entry : queued.transferred_ports) {
        DeleteRefIfPresent(env, &entry.source_port_ref);
      }
      return;
    }
  }
  EnqueueQueuedMessage(env, target, std::move(queued));
}

void EnqueueSerializedMessageToPort(
    napi_env env,
    const EdgeMessagePortDataPtr& target,
    void* payload_data,
    bool is_close,
    MessagePortWrap* close_source_wrap = nullptr,
    std::vector<QueuedMessage::TransferredPortEntry> transferred_ports = {}) {
  if (!target) {
    if (payload_data != nullptr) {
      unofficial_napi_release_serialized_value(payload_data);
    }
    for (auto& entry : transferred_ports) {
      DeleteRefIfPresent(env, &entry.source_port_ref);
    }
    return;
  }
  QueuedMessage queued;
  queued.payload_data = payload_data;
  queued.is_close = is_close;
  queued.close_source = close_source_wrap;
  queued.transferred_ports = std::move(transferred_ports);
  EnqueueQueuedMessage(env, target, std::move(queued));
}

void BeginClosePort(napi_env env, MessagePortWrap* wrap, bool notify_peer);
void DisentanglePeer(napi_env env, MessagePortWrap* wrap, bool enqueue_close);
void OnMessagePortClosed(uv_handle_t* handle);
void EmitMessageToPort(napi_env env,
                       napi_value port,
                       napi_value payload,
                       const char* type = "message",
                       napi_value ports = nullptr);

void ProcessQueuedMessages(MessagePortWrap* wrap, bool force, size_t processing_limit = std::numeric_limits<size_t>::max()) {
  if (wrap == nullptr || wrap->handle_wrap.env == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) {
    return;
  }

  while (processing_limit-- > 0) {
    QueuedMessage next;
    bool have_message = false;
    {
      if (!wrap->data) break;
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      if (wrap->data->queued_messages.empty()) break;
      if (!force && !wrap->receiving_messages && !wrap->data->queued_messages.front().is_close) break;
      next = wrap->data->queued_messages.front();
      wrap->data->queued_messages.pop_front();
      if (next.is_close) wrap->data->close_message_enqueued = false;
      have_message = true;
    }
    if (!have_message) break;

    if (next.is_close) {
      wrap->closing_has_ref = false;
      if (next.payload_data != nullptr) {
        unofficial_napi_release_serialized_value(next.payload_data);
        next.payload_data = nullptr;
      }
      BeginClosePort(wrap->handle_wrap.env, wrap, false);
      break;
    }

    napi_value self = EdgeHandleWrapGetRefValue(wrap->handle_wrap.env, wrap->handle_wrap.wrapper_ref);
    napi_value payload = nullptr;
    napi_value message_error = nullptr;
    if (next.payload_data != nullptr) {
      if (unofficial_napi_deserialize_value(wrap->handle_wrap.env, next.payload_data, &payload) != napi_ok) {
        payload = nullptr;
        message_error = TakePendingException(wrap->handle_wrap.env);
        if (message_error == nullptr) {
          message_error = CreateErrorWithMessage(wrap->handle_wrap.env, nullptr, "Message could not be deserialized");
        }
      }
      unofficial_napi_release_serialized_value(next.payload_data);
      next.payload_data = nullptr;
    }
    if (self != nullptr && message_error == nullptr) {
      napi_value ports = nullptr;
      if (!next.transferred_ports.empty()) {
        ReceivedTransferredPortState received_ports;
        std::vector<ValueTransformPair> seen_pairs;
        payload = RestoreTransferredPortsInValue(
            wrap->handle_wrap.env,
            payload != nullptr ? payload : Undefined(wrap->handle_wrap.env),
            next,
            &received_ports,
            &seen_pairs);
        message_error = TakePendingException(wrap->handle_wrap.env);
        if (message_error == nullptr) {
          ports = BuildTransferredPortsArray(wrap->handle_wrap.env, next, &received_ports);
        }
      }
      if (message_error == nullptr) {
        payload = RestoreTransferableDataAfterStructuredClone(
            wrap->handle_wrap.env,
            payload != nullptr ? payload : Undefined(wrap->handle_wrap.env));
        message_error = TakePendingException(wrap->handle_wrap.env);
      }
      if (message_error == nullptr) {
        DeleteTransferredPortRefs(wrap->handle_wrap.env, &next.transferred_ports);
        EmitMessageToPort(wrap->handle_wrap.env,
                          self,
                          payload != nullptr ? payload : Undefined(wrap->handle_wrap.env),
                          "message",
                          ports);
        continue;
      }
    }
    DeleteTransferredPortRefs(wrap->handle_wrap.env, &next.transferred_ports);
    if (self != nullptr) {
      EmitMessageToPort(wrap->handle_wrap.env,
                        self,
                        message_error != nullptr ? message_error : Undefined(wrap->handle_wrap.env),
                        "messageerror");
    }
  }

  if (!force && wrap->data) {
    bool has_queued_messages = false;
    {
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      has_queued_messages = !wrap->data->queued_messages.empty();
    }
    if (has_queued_messages) {
      TriggerPortAsync(wrap->data);
    }
  }
}

void DeleteMessagePortWrap(void* data) {
  auto* wrap = static_cast<MessagePortWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->async_id > 0) {
    EdgeAsyncWrapQueueDestroyId(wrap->handle_wrap.env, wrap->async_id);
    wrap->async_id = 0;
  }
  DeleteQueuedMessages(wrap->handle_wrap.env, wrap);
  delete wrap;
}

void OnMessagePortClosed(uv_handle_t* handle) {
  auto* wrap = static_cast<MessagePortWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr) return;
  // MessagePort-specific teardown that must happen as the handle closes.
  if (EdgeHandleWrapEnvCleanupStarted(wrap->handle_wrap.env)) {
    wrap->handle_wrap.delete_on_close = true;
  }
  DisentanglePeer(wrap->handle_wrap.env, wrap, true);
  if (wrap->data) {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    if (wrap->data->attached_port == wrap) {
      wrap->data->attached_port = nullptr;
    }
  }
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteMessagePortWrap);
}

void OnMessagePortAsync(uv_async_t* handle) {
  auto* wrap = static_cast<MessagePortWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr || wrap->handle_wrap.env == nullptr) return;
  edge::HandleScope scope(wrap->handle_wrap.env);
  if (!scope.is_open()) return;

  size_t processing_limit = 1000;
  if (wrap->data) {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    processing_limit = std::max<size_t>(wrap->data->queued_messages.size(), 1000);
  }
  ProcessQueuedMessages(wrap, false, processing_limit);
}

void DisentanglePeer(napi_env env, MessagePortWrap* wrap, bool enqueue_close) {
  if (wrap == nullptr) return;
  RemoveFromBroadcastChannelGroup(wrap->data);
  EdgeMessagePortDataPtr peer;
  if (wrap->data) {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    peer = wrap->data->sibling.lock();
    wrap->data->sibling.reset();
    wrap->data->closed = true;
  }
  if (peer) {
    {
      std::lock_guard<std::mutex> peer_lock(peer->mutex);
      peer->sibling.reset();
    }
    if (enqueue_close) {
      EnqueueMessageToPort(env, peer, nullptr, true, wrap);
    }
  }
}

void BeginClosePort(napi_env env, MessagePortWrap* wrap, bool notify_peer) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  if (notify_peer) {
    DisentanglePeer(env, wrap, true);
  }
  wrap->closing_has_ref =
      EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->async));
  wrap->handle_wrap.state = kEdgeHandleClosing;
  uv_close(reinterpret_cast<uv_handle_t*>(&wrap->async), OnMessagePortClosed);
}

void CloseMessagePortForCleanup(void* data) {
  auto* wrap = static_cast<MessagePortWrap*>(data);
  if (wrap == nullptr) return;
  wrap->handle_wrap.delete_on_close = true;
  BeginClosePort(wrap->handle_wrap.env, wrap, false);
}

void MessagePortFinalize(napi_env env, void* data, void* /*hint*/) {
  EdgeHandleWrapFinalizeNative(env, &static_cast<MessagePortWrap*>(data)->handle_wrap, data, DeleteMessagePortWrap);
}

void InvokePortSymbolHook(napi_env env, napi_value port, napi_value symbol) {
  if (port == nullptr || symbol == nullptr) return;
  napi_value hook = nullptr;
  if (napi_get_property(env, port, symbol, &hook) != napi_ok || !IsFunction(env, hook)) return;
  napi_value ignored = nullptr;
  if (EdgeMakeCallback(env, port, hook, 0, nullptr, &ignored) != napi_ok) {
    ClearPendingException(env);
  }
}

void EmitMessageToPort(napi_env env, napi_value port, napi_value payload, const char* type, napi_value ports) {
  if (port == nullptr) return;

  napi_value emit_message = ResolveEmitMessageValue(env);
  napi_value undefined = Undefined(env);
  napi_value type_value = nullptr;
  if (emit_message != nullptr &&
      napi_create_string_utf8(env, type != nullptr ? type : "message", NAPI_AUTO_LENGTH, &type_value) == napi_ok &&
      type_value != nullptr) {
    napi_value argv[3] = {
        payload != nullptr ? payload : undefined,
        ports != nullptr ? ports : undefined,
        type_value,
    };
    napi_value ignored = nullptr;
    if (EdgeMakeCallback(env, port, emit_message, 3, argv, &ignored) == napi_ok) {
      return;
    }
    ClearPendingException(env);
  }

  if (TryHybridDispatchMessageToPort(env, port, payload, type, ports)) {
    return;
  }

  napi_value event = nullptr;
  if (napi_create_object(env, &event) != napi_ok || event == nullptr) {
    return;
  }
  if (ports == nullptr) napi_create_array_with_length(env, 0, &ports);
  napi_set_named_property(env, event, "data", payload != nullptr ? payload : Undefined(env));
  napi_set_named_property(env, event, "target", port);
  napi_set_named_property(env, event, "ports", ports != nullptr ? ports : Undefined(env));
  type_value = nullptr;
  if (napi_create_string_utf8(env, type != nullptr ? type : "message", NAPI_AUTO_LENGTH, &type_value) == napi_ok &&
      type_value != nullptr) {
    napi_set_named_property(env, event, "type", type_value);
  }

  const char* handler_name = (type != nullptr && strcmp(type, "messageerror") == 0)
                                 ? "onmessageerror"
                                 : "onmessage";
  napi_value handler = GetNamed(env, port, handler_name);
  if (IsFunction(env, handler)) {
    napi_value ignored = nullptr;
    napi_value argv[1] = {event};
    if (EdgeMakeCallback(env, port, handler, 1, argv, &ignored) != napi_ok) {
      ClearPendingException(env);
    }
  }
}

void ConnectPorts(napi_env env, napi_value first, napi_value second) {
  MessagePortWrap* first_wrap = UnwrapMessagePort(env, first);
  MessagePortWrap* second_wrap = UnwrapMessagePort(env, second);
  if (first_wrap == nullptr || second_wrap == nullptr) return;
  if (!first_wrap->data) first_wrap->data = InternalCreateMessagePortData();
  if (!second_wrap->data) second_wrap->data = InternalCreateMessagePortData();
  InternalEntangleMessagePortData(first_wrap->data, second_wrap->data);
}

bool EnsureMessagingSymbols(napi_env env) {
  const bool already_cached = WithExistingMessagingState(
      env,
      [](MessagingState* state) {
        return state != nullptr &&
               state->no_message_symbol_ref != nullptr &&
               state->oninit_symbol_ref != nullptr;
      });
  if (already_cached) {
    return true;
  }
  napi_value symbols = edge::binding_registry::Get(env, "symbols");
  if (symbols == nullptr || IsUndefined(env, symbols)) return false;

  SetMessagingStateRefValue(env, &MessagingState::no_message_symbol_ref, GetNamed(env, symbols, "no_message_symbol"));
  SetMessagingStateRefValue(env, &MessagingState::oninit_symbol_ref, GetNamed(env, symbols, "oninit"));

  return WithExistingMessagingState(
      env,
      [](MessagingState* state) {
        return state != nullptr && state->no_message_symbol_ref != nullptr;
      });
}

napi_value MessagePortConstructorCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) {
    return nullptr;
  }

  if (!IsInternalMessagePortCtorCall(env, argc, argv)) {
    ThrowTypeErrorWithCode(env, "ERR_CONSTRUCT_CALL_INVALID", "Illegal constructor");
    return nullptr;
  }

  if (this_arg == nullptr) {
    return nullptr;
  }

  auto* wrap = new MessagePortWrap();
  EdgeHandleWrapInit(&wrap->handle_wrap, env);
  wrap->data = InternalCreateMessagePortData();
  if (napi_wrap(env, this_arg, wrap, MessagePortFinalize, nullptr, &wrap->handle_wrap.wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  (void)napi_type_tag_object(env, this_arg, &kMessagePortTypeTag);

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  const int rc = loop != nullptr ? uv_async_init(loop, &wrap->async, OnMessagePortAsync) : UV_EINVAL;
  if (rc == 0) {
    wrap->async.data = wrap;
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->async));
    wrap->handle_wrap.state = kEdgeHandleInitialized;
    EdgeHandleWrapAttach(&wrap->handle_wrap,
                        wrap,
                        reinterpret_cast<uv_handle_t*>(&wrap->async),
                        CloseMessagePortForCleanup);
    // Keep-alive comes from EdgeRegisterActiveHandle's strong keepalive_ref.
    wrap->async_id = EdgeAsyncWrapNextId(env);
    EdgeAsyncWrapEmitInit(
        env, wrap->async_id, kEdgeProviderMessagePort, EdgeAsyncWrapExecutionAsyncId(env), this_arg);
    {
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      wrap->data->attached_port = wrap;
    }
    wrap->handle_wrap.active_handle_token =
        EdgeRegisterActiveHandle(env,
                                 this_arg,
                                 "MESSAGEPORT",
                                 MessagePortHasRefActive,
                                 MessagePortGetActiveOwner,
                                 wrap,
                                 CloseMessagePortForCleanup);
  }

  const napi_value oninit_symbol = GetOnInitSymbol(env);
  if (rc == 0 && oninit_symbol != nullptr) {
    InvokePortSymbolHook(env, this_arg, oninit_symbol);
  }
  return this_arg;
}

napi_value MessagePortPostMessageCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;

  napi_value normalized_transfer_arg = nullptr;
  napi_value transfer_list = nullptr;
  if (argc >= 2 && argv[1] != nullptr &&
      !NormalizePostMessageTransferArg(env, argv[1], &normalized_transfer_arg, &transfer_list)) {
    return nullptr;
  }
  napi_value payload = (argc >= 1 && argv[0] != nullptr) ? argv[0] : Undefined(env);
  if (transfer_list != nullptr && TransferListContainsMarkedUntransferable(env, transfer_list, payload)) {
    napi_value err = CreateDataCloneError(env, "An ArrayBuffer is marked as untransferable");
    if (err != nullptr) {
      napi_throw(env, err);
    }
    return nullptr;
  }
  if (TransferListContainsDuplicateArrayBuffer(env, transfer_list)) {
    napi_value err = CreateDataCloneError(env, "Transfer list contains duplicate ArrayBuffer");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }
  if (!ValidateTransferListMessagePorts(env, transfer_list)) {
    return nullptr;
  }
  if (TransferListContainsDuplicateMessagePort(env, transfer_list)) {
    napi_value err = CreateDataCloneError(env, "Transfer list contains duplicate MessagePort");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }
  if (TransferListContainsMessagePort(env, transfer_list, this_arg)) {
    napi_value err = CreateDataCloneError(env, "Transfer list contains source port");
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }

  if (!EnsureNoMissingTransferredMessagePorts(env, payload, normalized_transfer_arg)) {
    return nullptr;
  }
  if (!EnsureNoMarkedUncloneableValue(env, payload)) {
    return nullptr;
  }
  if (ThrowDirectCloneFailureIfDetected(env, payload)) {
    return nullptr;
  }
  std::vector<QueuedMessage::TransferredPortEntry> transferred_ports;
  napi_value cloned_payload = nullptr;
  void* serialized_payload = nullptr;
  if (transfer_list != nullptr &&
      TransferListContainsValue(env, transfer_list, payload) &&
      IsTransferableValue(env, payload)) {
    if (!TransferRootJSTransferableValueForQueue(env, payload, &cloned_payload, &transferred_ports)) {
      DeleteTransferredPortRefs(env, &transferred_ports);
      return nullptr;
    }
  } else {
    if (!CollectTransferredPorts(env, transfer_list, &transferred_ports)) {
      return nullptr;
    }

    std::vector<ValueTransformPair> transferred_value_pairs;
    napi_value transferred_payload =
        TransformTransferredValuesForQueue(env, payload, transfer_list, &transferred_ports, &transferred_value_pairs);
    if (transferred_payload == nullptr) {
      DeleteTransferredPortRefs(env, &transferred_ports);
      return nullptr;
    }

    std::vector<ValueTransformPair> seen_pairs;
    napi_value transformed_payload =
        TransformTransferredPortsForQueue(env, transferred_payload, transferred_ports, &seen_pairs);
    if (transformed_payload == nullptr) {
      DeleteTransferredPortRefs(env, &transferred_ports);
      return nullptr;
    }

    EdgeMessagePortDataPtr fast_path_peer;
    if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized && wrap->data) {
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      fast_path_peer = wrap->data->sibling.lock();
    }

    napi_value arraybuffer_transfer_list = CreateArrayBufferTransferList(env, normalized_transfer_arg);
    if (fast_path_peer != nullptr && arraybuffer_transfer_list == nullptr) {
      if (!SerializeMessageValueForQueue(env, transformed_payload, &serialized_payload)) {
        DeleteTransferredPortRefs(env, &transferred_ports);
        if (serialized_payload != nullptr) {
          unofficial_napi_release_serialized_value(serialized_payload);
          serialized_payload = nullptr;
        }
        return ReadPendingCloneErrorMessage(env, transformed_payload);
      }
    } else {
      cloned_payload = CloneMessageValue(env, transformed_payload, normalized_transfer_arg);
      if (cloned_payload == nullptr) {
        DeleteTransferredPortRefs(env, &transferred_ports);
        return nullptr;
      }
    }
  }
  ClearPendingException(env);

  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized || !wrap->data) {
    if (serialized_payload != nullptr) {
      unofficial_napi_release_serialized_value(serialized_payload);
      serialized_payload = nullptr;
    }
    DetachTransferredPorts(env, &transferred_ports);
    return Undefined(env);
  }

  EdgeMessagePortDataPtr peer;
  {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    peer = wrap->data->sibling.lock();
  }
  if (peer) {
    bool target_in_transfer_list = false;
    for (const auto& entry : transferred_ports) {
      if (entry.data && entry.data.get() == peer.get()) {
        target_in_transfer_list = true;
        break;
      }
    }
    if (target_in_transfer_list) {
      if (serialized_payload != nullptr) {
        unofficial_napi_release_serialized_value(serialized_payload);
        serialized_payload = nullptr;
      }
      DetachTransferredPorts(env, &transferred_ports);
      EmitProcessWarning(env, "The target port was posted to itself, and the communication channel was lost");
      BeginClosePort(env, wrap, false);
      napi_value true_value = nullptr;
      napi_get_boolean(env, true, &true_value);
      return true_value;
    }

    DetachTransferredPorts(env, &transferred_ports);
    if (serialized_payload != nullptr) {
      EnqueueSerializedMessageToPort(
          env, peer, serialized_payload, false, nullptr, std::move(transferred_ports));
      serialized_payload = nullptr;
    } else {
      EnqueueMessageToPort(env, peer, cloned_payload, false, nullptr, std::move(transferred_ports));
    }
    napi_value true_value = nullptr;
    napi_get_boolean(env, true, &true_value);
    return true_value;
  }

  std::vector<EdgeMessagePortDataPtr> broadcast_targets = GetBroadcastChannelTargets(wrap->data);
  if (!broadcast_targets.empty()) {
    if (broadcast_targets.size() > 1 && !transferred_ports.empty()) {
      DeleteTransferredPortRefs(env, &transferred_ports);
      napi_value err = CreateDataCloneError(env, "Transferables cannot be used with multiple destinations");
      if (err != nullptr) napi_throw(env, err);
      return nullptr;
    }
    if (!transferred_ports.empty()) {
      DetachTransferredPorts(env, &transferred_ports);
    }
    for (size_t i = 0; i < broadcast_targets.size(); ++i) {
      std::vector<QueuedMessage::TransferredPortEntry> per_target_ports;
      if (i == 0) {
        per_target_ports = std::move(transferred_ports);
      }
      EnqueueMessageToPort(env, broadcast_targets[i], cloned_payload, false, nullptr, std::move(per_target_ports));
    }
    napi_value true_value = nullptr;
    napi_get_boolean(env, true, &true_value);
    return true_value;
  }

  if (!transferred_ports.empty()) {
    DetachTransferredPorts(env, &transferred_ports);
  }
  if (serialized_payload != nullptr) {
    unofficial_napi_release_serialized_value(serialized_payload);
    serialized_payload = nullptr;
  }
  napi_value false_value = nullptr;
  napi_get_boolean(env, false, &false_value);
  return false_value;
}

napi_value MessagePortStartCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    wrap->receiving_messages = true;
    TriggerPortAsync(wrap);
  }
  return Undefined(env);
}

napi_value MessagePortCloseCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;
  BeginClosePort(env, wrap, false);
  return Undefined(env);
}

napi_value MessagePortRefCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->async));
  }
  return Undefined(env);
}

napi_value MessagePortUnrefCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->async));
  }
  return Undefined(env);
}

napi_value MessagePortHasRefCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePortThisOrThrow(env, this_arg);
  if (wrap == nullptr) return nullptr;
  if (wrap->handle_wrap.state != kEdgeHandleInitialized) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  const bool has_ref = MessagePortHasRefActive(wrap);
  napi_get_boolean(env, has_ref, &out);
  return out;
}

napi_value CreateInternalMessagePortInstance(napi_env env) {
  napi_value ctor = WithMessagingStateRefValue(
      env,
      &MessagingState::message_port_ctor_ref,
      [](napi_value value) { return value; });
  napi_value token = WithMessagingStateRefValue(
      env,
      &MessagingState::message_port_ctor_token_ref,
      [](napi_value value) { return value; });
  if (!IsFunction(env, ctor) || token == nullptr) return nullptr;

  napi_value port = nullptr;
  napi_value argv[1] = {token};
  if (napi_new_instance(env, ctor, 1, argv, &port) != napi_ok || port == nullptr) {
    return nullptr;
  }
  return port;
}

napi_value MessageChannelConstructorCallback(napi_env env, napi_callback_info info) {
  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) == napi_ok && new_target == nullptr) {
    ThrowTypeErrorWithCode(env, "ERR_CONSTRUCT_CALL_REQUIRED", "Class constructor MessageChannel cannot be invoked without 'new'");
    return nullptr;
  }

  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  napi_value port1 = CreateInternalMessagePortInstance(env);
  napi_value port2 = CreateInternalMessagePortInstance(env);
  if (port1 == nullptr || port2 == nullptr) return this_arg;

  ConnectPorts(env, port1, port2);
  napi_set_named_property(env, this_arg, "port1", port1);
  napi_set_named_property(env, this_arg, "port2", port2);
  return this_arg;
}

napi_value ExposeLazyDOMExceptionPropertyCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  napi_valuetype target_type = napi_undefined;
  if (napi_typeof(env, argv[0], &target_type) != napi_ok || target_type != napi_object) return Undefined(env);

  napi_property_descriptor desc = {};
  desc.utf8name = "DOMException";
  desc.getter = [](napi_env env, napi_callback_info info) -> napi_value {
    napi_value this_arg = nullptr;
    if (napi_get_cb_info(env, info, nullptr, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
      return Undefined(env);
    }
    napi_value dom_exception = ResolveDOMExceptionValue(env);
    if (dom_exception == nullptr || IsUndefined(env, dom_exception)) return Undefined(env);

    napi_property_descriptor value_desc = {};
    value_desc.utf8name = "DOMException";
    value_desc.value = dom_exception;
    value_desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    (void)napi_define_properties(env, this_arg, 1, &value_desc);
    return dom_exception;
  };
  desc.attributes = napi_configurable;
  napi_define_properties(env, argv[0], 1, &desc);
  return Undefined(env);
}

napi_value SetDeserializerCreateObjectFunctionCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  SetMessagingStateRefValue(
      env,
      &MessagingState::deserializer_create_object_ref,
      argc >= 1 && IsFunction(env, argv[0]) ? argv[0] : nullptr);
  return Undefined(env);
}

napi_value StructuredCloneCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  if (!EnsureNoMissingTransferredMessagePorts(env, argv[0], argc >= 2 ? argv[1] : nullptr)) {
    return nullptr;
  }
  if (!EnsureNoMarkedUncloneableValue(env, argv[0])) {
    return nullptr;
  }
  if (ThrowDirectCloneFailureIfDetected(env, argv[0])) {
    return nullptr;
  }
  return argc >= 2 && argv[1] != nullptr
             ? CloneMessageValueWithTransfers(env, argv[0], argv[1])
             : CloneMessageValue(env, argv[0], nullptr);
}

napi_value BroadcastChannelCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return Undefined(env);

  const std::string name = (argc >= 1 && argv[0] != nullptr) ? ValueToUtf8(env, argv[0]) : std::string();
  napi_value handle = CreateInternalMessagePortInstance(env);
  if (handle == nullptr) return Undefined(env);

  MessagePortWrap* wrap = UnwrapMessagePort(env, handle);
  if (wrap == nullptr || !wrap->data) return Undefined(env);
  AttachToBroadcastChannelGroup(wrap->data, GetOrCreateBroadcastChannelGroup(name));
  return handle;
}

napi_value DrainMessagePortCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  MessagePortWrap* wrap = UnwrapMessagePort(env, argv[0]);
  if (wrap == nullptr) return Undefined(env);
  ProcessQueuedMessages(wrap, true);
  return Undefined(env);
}

bool IsContextifiedObject(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_object) {
    return false;
  }

  napi_value private_symbols = EdgeGetPrivateSymbols(env);
  napi_value context_symbol =
      private_symbols != nullptr ? GetNamed(env, private_symbols, "contextify_context_private_symbol") : nullptr;
  if (context_symbol == nullptr) return false;

  bool has_symbol = false;
  if (napi_has_property(env, value, context_symbol, &has_symbol) != napi_ok || !has_symbol) {
    ClearPendingException(env);
    return false;
  }

  napi_value context_value = nullptr;
  if (napi_get_property(env, value, context_symbol, &context_value) != napi_ok || context_value == nullptr) {
    ClearPendingException(env);
    return false;
  }
  return !IsUndefined(env, context_value);
}

void DeleteNamedPropertyIfPresent(napi_env env, napi_value target, const char* name) {
  if (env == nullptr || target == nullptr || name == nullptr) return;
  napi_value key = nullptr;
  bool deleted = false;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &key) != napi_ok || key == nullptr) return;
  if (napi_delete_property(env, target, key, &deleted) != napi_ok) {
    ClearPendingException(env);
  }
}

napi_value PrepareMovedMessagePortOutgoingValue(
    napi_env env,
    napi_value value,
    napi_value native_port_symbol,
    napi_value contextify_context_symbol,
    std::vector<ValueTransformPair>* seen_pairs) {
  if (value == nullptr || IsNullOrUndefinedValue(env, value) || IsCloneByReferenceValue(env, value)) {
    return value;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return value;
  }

  if (native_port_symbol != nullptr) {
    bool has_native_port = false;
    if (napi_has_property(env, value, native_port_symbol, &has_native_port) == napi_ok && has_native_port) {
      napi_value native_port = nullptr;
      if (napi_get_property(env, value, native_port_symbol, &native_port) == napi_ok &&
          native_port != nullptr &&
          !IsUndefined(env, native_port)) {
        return native_port;
      }
      ClearPendingException(env);
    } else {
      ClearPendingException(env);
    }
  }

  if (contextify_context_symbol != nullptr) {
    bool is_context_object = false;
    if (napi_has_property(env, value, contextify_context_symbol, &is_context_object) == napi_ok &&
        is_context_object) {
      return value;
    }
    ClearPendingException(env);
  }

  if (seen_pairs != nullptr) {
    napi_value existing = FindTransformedValue(env, value, *seen_pairs);
    if (existing != nullptr) return existing;
  }

  const std::string tag = GetObjectTag(env, value);

  if (tag == "[object Array]") {
    uint32_t length = 0;
    if (napi_get_array_length(env, value, &length) != napi_ok) return nullptr;
    napi_value out = nullptr;
    if (napi_create_array_with_length(env, length, &out) != napi_ok || out == nullptr) return nullptr;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});
    for (uint32_t i = 0; i < length; ++i) {
      napi_value item = nullptr;
      if (napi_get_element(env, value, i, &item) != napi_ok) return nullptr;
      napi_value prepared = PrepareMovedMessagePortOutgoingValue(
          env, item, native_port_symbol, contextify_context_symbol, seen_pairs);
      if (prepared == nullptr || napi_set_element(env, out, i, prepared) != napi_ok) return nullptr;
    }
    return out;
  }

  if (tag == "[object Object]") {
    napi_value out = nullptr;
    if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;
    if (seen_pairs != nullptr) seen_pairs->push_back({value, out});

    napi_value keys = nullptr;
    if (napi_get_property_names(env, value, &keys) != napi_ok || keys == nullptr) return nullptr;
    uint32_t length = 0;
    if (napi_get_array_length(env, keys, &length) != napi_ok) return nullptr;
    for (uint32_t i = 0; i < length; ++i) {
      napi_value key = nullptr;
      if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) return nullptr;
      napi_value item = nullptr;
      if (napi_get_property(env, value, key, &item) != napi_ok) return nullptr;
      napi_value prepared = PrepareMovedMessagePortOutgoingValue(
          env, item, native_port_symbol, contextify_context_symbol, seen_pairs);
      if (prepared == nullptr || napi_set_property(env, out, key, prepared) != napi_ok) return nullptr;
    }
    return out;
  }

  return value;
}

napi_value MoveMessagePortPostMessageBridgeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 4 || argv[0] == nullptr || argv[1] == nullptr || argv[2] == nullptr) {
    ThrowIllegalInvocation(env);
    return nullptr;
  }

  napi_value native_port = argv[0];
  napi_value native_port_symbol = argv[1];
  napi_value contextify_context_symbol = argv[2];
  napi_value message = argv[3];
  napi_value transfer_list = argc >= 5 ? argv[4] : Undefined(env);

  std::vector<ValueTransformPair> seen_pairs;
  napi_value prepared_message = PrepareMovedMessagePortOutgoingValue(
      env, message, native_port_symbol, contextify_context_symbol, &seen_pairs);
  if (prepared_message == nullptr) return nullptr;

  napi_value prepared_transfer_list = transfer_list;
  if (transfer_list != nullptr && !IsUndefined(env, transfer_list)) {
    seen_pairs.clear();
    prepared_transfer_list = PrepareMovedMessagePortOutgoingValue(
        env, transfer_list, native_port_symbol, contextify_context_symbol, &seen_pairs);
    if (prepared_transfer_list == nullptr) return nullptr;
  }

  napi_value post_message = GetNamed(env, native_port, "postMessage");
  if (!IsFunction(env, post_message)) {
    ThrowIllegalInvocation(env);
    return nullptr;
  }

  napi_value call_argv[2] = {prepared_message, prepared_transfer_list};
  napi_value out = nullptr;
  const size_t call_argc =
      prepared_transfer_list == nullptr || IsUndefined(env, prepared_transfer_list) ? 1u : 2u;
  if (napi_call_function(env, native_port, post_message, call_argc, call_argv, &out) != napi_ok) {
    return nullptr;
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value CreateMovedMessagePortWrapperInContext(napi_env env,
                                                  napi_value contextified_object,
                                                  napi_value native_port) {
  if (env == nullptr || contextified_object == nullptr || native_port == nullptr) return nullptr;

  napi_value native_move_fn = nullptr;
  napi_value native_post_message_fn = nullptr;
  if (napi_create_function(env,
                           "moveMessagePortToContext",
                           NAPI_AUTO_LENGTH,
                           MoveMessagePortToContextCallback,
                           nullptr,
                           &native_move_fn) != napi_ok ||
      native_move_fn == nullptr ||
      napi_create_function(env,
                           "moveMessagePortPostMessage",
                           NAPI_AUTO_LENGTH,
                           MoveMessagePortPostMessageBridgeCallback,
                           nullptr,
                           &native_post_message_fn) != napi_ok ||
      native_post_message_fn == nullptr) {
    return nullptr;
  }

  napi_value native_ctor = WithMessagingStateRefValue(
      env,
      &MessagingState::message_port_ctor_ref,
      [](napi_value value) { return value; });
  napi_value native_key_object_ctor = GetCryptoModuleCtor(env, "KeyObject");
  napi_value private_symbols = EdgeGetPrivateSymbols(env);
  napi_value transfer_mode_symbol =
      private_symbols != nullptr ? GetNamed(env, private_symbols, "transfer_mode_private_symbol") : nullptr;
  napi_value contextify_context_symbol =
      private_symbols != nullptr ? GetNamed(env, private_symbols, "contextify_context_private_symbol") : nullptr;
  if (native_ctor == nullptr || transfer_mode_symbol == nullptr || contextify_context_symbol == nullptr ||
      native_key_object_ctor == nullptr) {
    return nullptr;
  }

  constexpr const char* kTempNativePort = "__ubiMoveMessagePortNativePort__";
  constexpr const char* kTempNativeMove = "__ubiMoveMessagePortNativeMove__";
  constexpr const char* kTempNativePostMessage = "__ubiMoveMessagePortNativePostMessage__";
  constexpr const char* kTempNativeCtor = "__ubiMoveMessagePortNativeCtor__";
  constexpr const char* kTempNativeKeyObjectCtor = "__ubiMoveMessagePortNativeKeyObjectCtor__";
  constexpr const char* kTempTransferMode = "__ubiMoveMessagePortTransferModeSymbol__";
  constexpr const char* kTempContextifyContext = "__ubiMoveMessagePortContextifyContextSymbol__";

  napi_set_named_property(env, contextified_object, kTempNativePort, native_port);
  napi_set_named_property(env, contextified_object, kTempNativeMove, native_move_fn);
  napi_set_named_property(env, contextified_object, kTempNativePostMessage, native_post_message_fn);
  napi_set_named_property(env, contextified_object, kTempNativeCtor, native_ctor);
  napi_set_named_property(env, contextified_object, kTempNativeKeyObjectCtor, native_key_object_ctor);
  napi_set_named_property(env, contextified_object, kTempTransferMode, transfer_mode_symbol);
  napi_set_named_property(env, contextified_object, kTempContextifyContext, contextify_context_symbol);

  constexpr const char* kWrapperSource = R"JS(
(() => {
  const nativePort = globalThis.__ubiMoveMessagePortNativePort__;
  const nativeMove = globalThis.__ubiMoveMessagePortNativeMove__;
  const nativePostMessage = globalThis.__ubiMoveMessagePortNativePostMessage__;
  const NativeMessagePort = globalThis.__ubiMoveMessagePortNativeCtor__;
  const NativeKeyObject = globalThis.__ubiMoveMessagePortNativeKeyObjectCtor__;
  const transferModeSymbol =
    globalThis.__ubiMoveMessagePortTransferModeSymbol__;
  const contextifyContextSymbol =
    globalThis.__ubiMoveMessagePortContextifyContextSymbol__;
  delete globalThis.__ubiMoveMessagePortNativePort__;
  delete globalThis.__ubiMoveMessagePortNativeMove__;
  delete globalThis.__ubiMoveMessagePortNativePostMessage__;
  delete globalThis.__ubiMoveMessagePortNativeCtor__;
  delete globalThis.__ubiMoveMessagePortNativeKeyObjectCtor__;
  delete globalThis.__ubiMoveMessagePortTransferModeSymbol__;
  delete globalThis.__ubiMoveMessagePortContextifyContextSymbol__;

  const nativePortSymbol =
    globalThis.__ubiMovedMessagePortNativeSymbol__ ||
    (globalThis.__ubiMovedMessagePortNativeSymbol__ =
      Symbol('nodejs.movedMessagePort.native'));
  const portCache =
    globalThis.__ubiMovedMessagePortCache__ ||
    (globalThis.__ubiMovedMessagePortCache__ = new WeakMap());

  if (portCache.has(nativePort))
    return portCache.get(nativePort);

  function makeContextUnavailableError() {
    const err = new Error('ERR_MESSAGE_TARGET_CONTEXT_UNAVAILABLE');
    err.code = 'ERR_MESSAGE_TARGET_CONTEXT_UNAVAILABLE';
    return err;
  }

  function wrapThrownError(err) {
    const DomExceptionCtor =
      typeof DOMException === 'function'
        ? DOMException
        : class DOMException extends Error {};
    if (err?.name === 'DataCloneError' ||
        err?.constructor?.name === 'DOMException') {
      const out = new DomExceptionCtor(String(err.message), String(err.name));
      out.name = String(err.name);
      if (err && 'code' in err) {
        try {
          Object.defineProperty(out, 'code', {
            __proto__: null,
            value: err.code,
            configurable: true,
          });
        } catch {}
      }
      return out;
    }
    const out = new Error(err?.message == null ? String(err) : String(err.message));
    if (err?.name)
      out.name = err.name;
    if (err && 'code' in err)
      out.code = err.code;
    return out;
  }

  function objectTag(value) {
    return Object.prototype.toString.call(value);
  }

  function isRawNativePort(value) {
    return value instanceof NativeMessagePort;
  }

  function isMovedWrapper(value) {
    return value !== null &&
      (typeof value === 'object' || typeof value === 'function') &&
      value[nativePortSymbol] !== undefined;
  }

  function isUnsupportedTransferable(value) {
    return value !== null &&
      (typeof value === 'object' || typeof value === 'function') &&
      ((transferModeSymbol !== undefined &&
        value[transferModeSymbol] !== undefined) ||
       (typeof NativeKeyObject === 'function' && value instanceof NativeKeyObject)) &&
      !isRawNativePort(value);
  }

  function moveNativePort(value) {
    if (portCache.has(value))
      return portCache.get(value);
    const moved = nativeMove(value, globalThis);
    portCache.set(value, moved);
    return moved;
  }

  function transformIncoming(value, seen = new WeakMap()) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function'))
      return value;
    if (isRawNativePort(value))
      return moveNativePort(value);
    if (isUnsupportedTransferable(value))
      throw makeContextUnavailableError();
    if (seen.has(value))
      return seen.get(value);

    const tag = objectTag(value);
    if (tag === '[object Array]') {
      const out = [];
      seen.set(value, out);
      for (let i = 0; i < value.length; i++)
        out[i] = transformIncoming(value[i], seen);
      return out;
    }
    if (tag === '[object Map]') {
      const out = new Map();
      seen.set(value, out);
      for (const [key, entryValue] of value)
        out.set(transformIncoming(key, seen), transformIncoming(entryValue, seen));
      return out;
    }
    if (tag === '[object Set]') {
      const out = new Set();
      seen.set(value, out);
      for (const entryValue of value)
        out.add(transformIncoming(entryValue, seen));
      return out;
    }
    if (tag === '[object Object]') {
      const out = {};
      seen.set(value, out);
      for (const key of Reflect.ownKeys(value)) {
        const desc = Object.getOwnPropertyDescriptor(value, key);
        if (!desc)
          continue;
        if ('value' in desc)
          desc.value = transformIncoming(desc.value, seen);
        Object.defineProperty(out, key, desc);
      }
      return out;
    }
    return value;
  }

  const proto = {
    postMessage(message, transferList) {
      try {
        return nativePostMessage(
          nativePort,
          nativePortSymbol,
          contextifyContextSymbol,
          message,
          transferList);
      } catch (err) {
        throw wrapThrownError(err);
      }
    },
    start() {
      return nativePort.start();
    },
    close(cb) {
      return nativePort.close(cb);
    },
    ref() {
      return nativePort.ref();
    },
    unref() {
      return nativePort.unref();
    },
    hasRef() {
      return nativePort.hasRef();
    },
  };

  const wrapper = Object.create(proto);
  Object.defineProperty(wrapper, nativePortSymbol, {
    __proto__: null,
    configurable: false,
    enumerable: false,
    writable: false,
    value: nativePort,
  });
  wrapper.onmessage = undefined;
  wrapper.onmessageerror = undefined;

  portCache.set(nativePort, wrapper);

  nativePort.onmessage = (event) => {
    try {
      const seen = new WeakMap();
      const data = transformIncoming(event.data, seen);
      const ports = transformIncoming(event.ports ?? [], seen);
      if (typeof wrapper.onmessage === 'function') {
        wrapper.onmessage({
          __proto__: null,
          data,
          ports,
          target: wrapper,
          type: 'message',
        });
      }
    } catch (err) {
      if (typeof wrapper.onmessageerror === 'function') {
        wrapper.onmessageerror({
          __proto__: null,
          data: err,
          ports: [],
          target: wrapper,
          type: 'messageerror',
        });
      }
    }
  };

  nativePort.onmessageerror = (event) => {
    let data = event.data;
    try {
      data = transformIncoming(data, new WeakMap());
    } catch (err) {
      data = err;
    }
    if (typeof wrapper.onmessageerror === 'function') {
      wrapper.onmessageerror({
        __proto__: null,
        data,
        ports: [],
        target: wrapper,
        type: 'messageerror',
      });
    }
  };

  nativePort.unref();

  return wrapper;
})()
)JS";

  napi_value source = nullptr;
  napi_value filename = nullptr;
  napi_value undefined = Undefined(env);
  if (napi_create_string_utf8(env, kWrapperSource, NAPI_AUTO_LENGTH, &source) != napi_ok || source == nullptr ||
      napi_create_string_utf8(
          env, "node:internal/worker/move_message_port_context", NAPI_AUTO_LENGTH, &filename) != napi_ok ||
      filename == nullptr) {
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativePort);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeMove);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativePostMessage);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeCtor);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeKeyObjectCtor);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempTransferMode);
    DeleteNamedPropertyIfPresent(env, contextified_object, kTempContextifyContext);
    return nullptr;
  }

  napi_value out = nullptr;
  const unofficial_napi_js_source run_source{source, nullptr};
  const napi_status status = unofficial_napi_contextify_run_script(env,
                                                                   contextified_object,
                                                                   &run_source,
                                                                   filename,
                                                                   0,
                                                                   0,
                                                                   -1,
                                                                   true,
                                                                   false,
                                                                   false,
                                                                   undefined,
                                                                   &out);

  DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativePort);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeMove);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativePostMessage);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeCtor);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempNativeKeyObjectCtor);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempTransferMode);
  DeleteNamedPropertyIfPresent(env, contextified_object, kTempContextifyContext);

  if (status != napi_ok) return nullptr;
  return out;
}

napi_value MoveMessagePortToContextCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"port\" argument must be a MessagePort instance");
    return nullptr;
  }

  MessagePortWrap* wrap = UnwrapMessagePort(env, argv[0]);
  if (wrap == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"port\" argument must be a MessagePort instance");
    return nullptr;
  }
  if (wrap->handle_wrap.state != kEdgeHandleInitialized || !wrap->data) {
    ThrowClosedMessagePortError(env);
    return nullptr;
  }

  bool closed = false;
  {
    std::lock_guard<std::mutex> lock(wrap->data->mutex);
    closed = wrap->data->closed;
  }
  if (closed) {
    ThrowClosedMessagePortError(env);
    return nullptr;
  }

  if (argc < 2 || argv[1] == nullptr) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "Invalid context argument");
    return nullptr;
  }
  if (!IsContextifiedObject(env, argv[1])) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "Invalid context argument");
    return nullptr;
  }

  napi_value wrapper = CreateMovedMessagePortWrapperInContext(env, argv[1], argv[0]);
  return wrapper != nullptr ? wrapper : nullptr;
}

napi_value ReceiveMessageOnPortCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || argv[0] == nullptr) {
    napi_value symbol = GetNoMessageSymbol(env);
    return symbol != nullptr ? symbol : Undefined(env);
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, argv[0], &type) != napi_ok || type != napi_object) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"port\" argument must be a MessagePort instance");
    return nullptr;
  }
  MessagePortWrap* wrap = UnwrapMessagePort(env, argv[0]);
  if (wrap == nullptr) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"port\" argument must be a MessagePort instance");
    return nullptr;
  }

  QueuedMessage next;
  bool have_message = false;
  {
    if (wrap->data) {
      std::lock_guard<std::mutex> lock(wrap->data->mutex);
      if (!wrap->data->queued_messages.empty()) {
        next = wrap->data->queued_messages.front();
        wrap->data->queued_messages.pop_front();
        if (next.is_close) wrap->data->close_message_enqueued = false;
        have_message = true;
      }
    }
  }
  if (!have_message) {
    napi_value symbol = GetNoMessageSymbol(env);
    return symbol != nullptr ? symbol : Undefined(env);
  }

  if (next.is_close) {
    if (next.payload_data != nullptr) {
      unofficial_napi_release_serialized_value(next.payload_data);
      next.payload_data = nullptr;
    }
    for (auto& entry : next.transferred_ports) {
      DeleteRefIfPresent(env, &entry.source_port_ref);
    }
    BeginClosePort(env, wrap, false);
    napi_value symbol = GetNoMessageSymbol(env);
    return symbol != nullptr ? symbol : Undefined(env);
  }

  napi_value value = nullptr;
  if (next.payload_data != nullptr) {
    if (unofficial_napi_deserialize_value(env, next.payload_data, &value) != napi_ok) {
      value = nullptr;
    }
    unofficial_napi_release_serialized_value(next.payload_data);
    next.payload_data = nullptr;
  }
  if (value == nullptr) {
    napi_value exception = TakePendingException(env);
    if (exception == nullptr) {
      exception = CreateErrorWithMessage(env, nullptr, "Message could not be deserialized");
    }
    DeleteTransferredPortRefs(env, &next.transferred_ports);
    if (exception != nullptr) napi_throw(env, exception);
    return nullptr;
  }
  napi_value exception = nullptr;
  if (!next.transferred_ports.empty()) {
    ReceivedTransferredPortState received_ports;
    std::vector<ValueTransformPair> seen_pairs;
    value = RestoreTransferredPortsInValue(
        env,
        value != nullptr ? value : Undefined(env),
        next,
        &received_ports,
        &seen_pairs);
    exception = TakePendingException(env);
    if (exception != nullptr) {
      DeleteTransferredPortRefs(env, &next.transferred_ports);
      napi_throw(env, exception);
      return nullptr;
    }
  }
  value = RestoreTransferableDataAfterStructuredClone(
      env,
      value != nullptr ? value : Undefined(env));
  exception = TakePendingException(env);
  if (exception != nullptr) {
    DeleteTransferredPortRefs(env, &next.transferred_ports);
    napi_throw(env, exception);
    return nullptr;
  }
  DeleteTransferredPortRefs(env, &next.transferred_ports);
  return value != nullptr ? value : Undefined(env);
}

napi_value StopMessagePortCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc >= 1 && argv[0] != nullptr) {
    MessagePortWrap* wrap = UnwrapMessagePort(env, argv[0]);
    if (wrap != nullptr) wrap->receiving_messages = false;
  }
  return Undefined(env);
}

napi_value ResolveDOMExceptionValue(napi_env env) {
  napi_value per_context_exports = EdgeGetPerContextExports(env);
  if (per_context_exports != nullptr && !IsUndefined(env, per_context_exports)) {
    napi_value dom_exception = GetNamed(env, per_context_exports, "DOMException");
    if (IsFunction(env, dom_exception)) return dom_exception;
  }

  napi_value global = GetGlobal(env);
  napi_value dom_exception = GetNamed(env, global, "DOMException");
  return IsFunction(env, dom_exception) ? dom_exception : Undefined(env);
}

napi_value ResolveEmitMessageValue(napi_env env) {
  napi_value cached = WithMessagingStateRefValue(
      env,
      &MessagingState::emit_message_ref,
      [](napi_value value) { return value; });
  if (IsFunction(env, cached)) return cached;

  napi_value per_context_exports = EdgeGetPerContextExports(env);
  if (per_context_exports == nullptr || IsUndefined(env, per_context_exports)) return nullptr;

  napi_value emit_message = GetNamed(env, per_context_exports, "emitMessage");
  if (!IsFunction(env, emit_message)) return nullptr;

  SetMessagingStateRefValue(env, &MessagingState::emit_message_ref, emit_message);
  return emit_message;
}

napi_value GetCachedMessaging(napi_env env) {
  return WithMessagingStateRefValue(
      env,
      &MessagingState::binding_ref,
      [](napi_value value) { return value; });
}

}  // namespace

EdgeMessagePortDataPtr EdgeCreateMessagePortData() {
  return InternalCreateMessagePortData();
}

void EdgeEntangleMessagePortData(const EdgeMessagePortDataPtr& first,
                                const EdgeMessagePortDataPtr& second) {
  InternalEntangleMessagePortData(first, second);
}

EdgeMessagePortDataPtr EdgeGetMessagePortData(napi_env env, napi_value value) {
  MessagePortWrap* wrap = UnwrapMessagePort(env, value);
  if (wrap == nullptr) return nullptr;
  return wrap->data;
}

napi_value EdgeCreateMessagePortForData(napi_env env, const EdgeMessagePortDataPtr& data) {
  if (!data) return Undefined(env);

  napi_value port = CreateInternalMessagePortInstance(env);
  if (port == nullptr) return Undefined(env);

  MessagePortWrap* wrap = UnwrapMessagePort(env, port);
  if (wrap == nullptr) return Undefined(env);

  if (wrap->data) {
    std::lock_guard<std::mutex> old_lock(wrap->data->mutex);
    if (wrap->data->attached_port == wrap) {
      wrap->data->attached_port = nullptr;
    }
  }

  wrap->data = data;
  bool has_queued_messages = false;
  {
    std::lock_guard<std::mutex> lock(data->mutex);
    data->attached_port = wrap;
    data->closed = false;
    has_queued_messages = !data->queued_messages.empty();
  }
  if (has_queued_messages) {
    TriggerPortAsync(wrap);
  }
  return port;
}

void EdgeCloseMessagePortForValue(napi_env env, napi_value value) {
  MessagePortWrap* wrap = UnwrapMessagePort(env, value);
  if (wrap == nullptr) return;
  BeginClosePort(env, wrap, false);
}

napi_value InitMessaging(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedMessaging(env);
  if (cached != nullptr) return cached;

  EnsureMessagingSymbols(env);

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  napi_value message_port_ctor_token = nullptr;
  if (napi_create_object(env, &message_port_ctor_token) == napi_ok && message_port_ctor_token != nullptr) {
    SetMessagingStateRefValue(env, &MessagingState::message_port_ctor_token_ref, message_port_ctor_token);
  }

  napi_value message_port_ctor = nullptr;
  if (napi_define_class(env,
                        "MessagePort",
                        NAPI_AUTO_LENGTH,
                        MessagePortConstructorCallback,
                        nullptr,
                        0,
                        nullptr,
                        &message_port_ctor) == napi_ok &&
      message_port_ctor != nullptr) {
    constexpr napi_property_attributes kMutableMethodAttrs =
        static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    napi_property_descriptor methods[] = {
        {"postMessage", nullptr, MessagePortPostMessageCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
        {"start", nullptr, MessagePortStartCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
        {"close", nullptr, MessagePortCloseCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
        {"ref", nullptr, MessagePortRefCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
        {"unref", nullptr, MessagePortUnrefCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
        {"hasRef", nullptr, MessagePortHasRefCallback, nullptr, nullptr, nullptr, kMutableMethodAttrs, nullptr},
    };
    napi_value prototype = nullptr;
    if (napi_get_named_property(env, message_port_ctor, "prototype", &prototype) == napi_ok && prototype != nullptr) {
      napi_define_properties(env, prototype, sizeof(methods) / sizeof(methods[0]), methods);
    }
    napi_set_named_property(env, out, "MessagePort", message_port_ctor);
    SetMessagingStateRefValue(env, &MessagingState::message_port_ctor_ref, message_port_ctor);
  }

  napi_value message_channel_ctor = nullptr;
  if (napi_define_class(env,
                        "MessageChannel",
                        NAPI_AUTO_LENGTH,
                        MessageChannelConstructorCallback,
                        nullptr,
                        0,
                        nullptr,
                        &message_channel_ctor) == napi_ok &&
      message_channel_ctor != nullptr) {
    napi_set_named_property(env, out, "MessageChannel", message_channel_ctor);
  }

  napi_value broadcast_channel_fn = nullptr;
  if (napi_create_function(env,
                           "broadcastChannel",
                           NAPI_AUTO_LENGTH,
                           BroadcastChannelCallback,
                           nullptr,
                           &broadcast_channel_fn) == napi_ok &&
      broadcast_channel_fn != nullptr) {
    napi_set_named_property(env, out, "broadcastChannel", broadcast_channel_fn);
  }

  napi_value drain_fn = nullptr;
  if (napi_create_function(env,
                           "drainMessagePort",
                           NAPI_AUTO_LENGTH,
                           DrainMessagePortCallback,
                           nullptr,
                           &drain_fn) == napi_ok &&
      drain_fn != nullptr) {
    napi_set_named_property(env, out, "drainMessagePort", drain_fn);
  }

  napi_value move_fn = nullptr;
  if (napi_create_function(env,
                           "moveMessagePortToContext",
                           NAPI_AUTO_LENGTH,
                           MoveMessagePortToContextCallback,
                           nullptr,
                           &move_fn) == napi_ok &&
      move_fn != nullptr) {
    napi_set_named_property(env, out, "moveMessagePortToContext", move_fn);
  }

  napi_value receive_fn = nullptr;
  if (napi_create_function(env,
                           "receiveMessageOnPort",
                           NAPI_AUTO_LENGTH,
                           ReceiveMessageOnPortCallback,
                           nullptr,
                           &receive_fn) == napi_ok &&
      receive_fn != nullptr) {
    napi_set_named_property(env, out, "receiveMessageOnPort", receive_fn);
  }

  napi_value stop_fn = nullptr;
  if (napi_create_function(env,
                           "stopMessagePort",
                           NAPI_AUTO_LENGTH,
                           StopMessagePortCallback,
                           nullptr,
                           &stop_fn) == napi_ok &&
      stop_fn != nullptr) {
    napi_set_named_property(env, out, "stopMessagePort", stop_fn);
  }

  napi_value expose_fn = nullptr;
  if (napi_create_function(env,
                           "exposeLazyDOMExceptionProperty",
                           NAPI_AUTO_LENGTH,
                           ExposeLazyDOMExceptionPropertyCallback,
                           nullptr,
                           &expose_fn) == napi_ok &&
      expose_fn != nullptr) {
    napi_set_named_property(env, out, "exposeLazyDOMExceptionProperty", expose_fn);
  }

  napi_value set_deserializer = nullptr;
  if (napi_create_function(env,
                           "setDeserializerCreateObjectFunction",
                           NAPI_AUTO_LENGTH,
                           SetDeserializerCreateObjectFunctionCallback,
                           nullptr,
                           &set_deserializer) == napi_ok &&
      set_deserializer != nullptr) {
    napi_set_named_property(env, out, "setDeserializerCreateObjectFunction", set_deserializer);
  }

  napi_value structured_clone = nullptr;
  if (napi_create_function(env,
                           "structuredClone",
                           NAPI_AUTO_LENGTH,
                           StructuredCloneCallback,
                           nullptr,
                           &structured_clone) == napi_ok &&
      structured_clone != nullptr) {
    napi_set_named_property(env, out, "structuredClone", structured_clone);
  }

  napi_value dom_exception = ResolveDOMExceptionValue(env);
  if (dom_exception != nullptr && !IsUndefined(env, dom_exception)) {
    napi_set_named_property(env, out, "DOMException", dom_exception);
  }

  SetMessagingStateRefValue(env, &MessagingState::binding_ref, out);
  return out;
}

}  // namespace internal_binding
