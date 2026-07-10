#include "internal_binding/binding_initializers.h"

#include "edge_trace.h"

#include <algorithm>
#include <cstddef>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fcntl.h>
#include <functional>
#include <limits>
#include <cstdio>
#include <sstream>
#include <string>
#include <sys/stat.h>
#include <unistd.h>
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <uv.h>
#include "ada.h"

#include "binding_registry/binding_registry.h"
#include "edge_environment.h"
#include "edge_fs.h"
#include "internal_binding/helpers.h"
#include "../edge_env_loop.h"
#include "../edge_handle_scope.h"
#include "../edge_handle_wrap.h"
#include "../edge_async_wrap.h"
#include "../edge_stream_base.h"
#include "../edge_stream_wrap.h"
#include "../edge_module_loader.h"
#include "../edge_path.h"
#include "../edge_worker_env.h"
#include "edge_active_resource.h"
#include "edge_runtime.h"

namespace internal_binding {

namespace {

constexpr size_t kFsStatsLength = 18;
constexpr size_t kFsStatFsLength = 7;

void ClearPendingException(napi_env env) {
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  napi_get_and_clear_last_exception(env, &ignored);
}

struct AsyncFsReq;

void ResetRef(napi_env env, napi_ref* ref_ptr);
void DestroyAsyncFsReq(AsyncFsReq* async_req);
void FinishAsyncFsReq(AsyncFsReq* async_req, int result);

struct FsBindingState {
  explicit FsBindingState(napi_env env_in) : env(env_in) {}
  ~FsBindingState();

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  std::unordered_map<std::string, napi_ref> raw_methods;
  std::unordered_multimap<std::string, AsyncFsReq*> pending_fifo_writer_opens;
  napi_ref file_handle_ctor_ref = nullptr;
  napi_ref fs_req_ctor_ref = nullptr;
  napi_ref stat_watcher_ctor_ref = nullptr;
  napi_ref k_use_promises_symbol_ref = nullptr;
};
int64_t g_next_stat_watcher_async_id = 700000;

FsBindingState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<FsBindingState>(env, kEdgeEnvironmentSlotFsBindingState);
}

FsBindingState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<FsBindingState>(
      env, kEdgeEnvironmentSlotFsBindingState);
}

std::filesystem::path ResolveSymlinkComponentsForModuleResolution(const std::filesystem::path& path) {
  std::filesystem::path current;
  for (const std::filesystem::path& part : path.lexically_normal()) {
    if (part == "." || part.empty()) continue;
    if (part == "..") {
      current /= part;
      continue;
    }
    if (part == path.root_name() || part == path.root_directory()) {
      current /= part;
      continue;
    }

    std::filesystem::path candidate = current.empty() ? part : current / part;
    std::error_code ec;
    if (std::filesystem::is_symlink(candidate, ec) && !ec) {
      std::filesystem::path target = std::filesystem::read_symlink(candidate, ec);
      if (!ec) {
        current = target.is_absolute() ? target : candidate.parent_path() / target;
        current = current.lexically_normal();
        continue;
      }
    }
    current = candidate;
  }
  return current.lexically_normal();
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

napi_value GetBinding(napi_env env) {
  FsBindingState* st = GetState(env);
  return st == nullptr ? nullptr : GetRefValue(env, st->binding_ref);
}

void ResetRef(napi_env env, napi_ref* ref_ptr) {
  if (ref_ptr == nullptr || *ref_ptr == nullptr) return;
  napi_delete_reference(env, *ref_ptr);
  *ref_ptr = nullptr;
}

void SetNamedMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void SetNamedInt(napi_env env, napi_value obj, const char* name, int32_t value) {
  napi_value v = nullptr;
  if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) napi_set_named_property(env, obj, name, v);
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

void DeleteNamedProperty(napi_env env, napi_value obj, const char* name) {
  if (env == nullptr || obj == nullptr || name == nullptr) return;
  napi_value key = nullptr;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &key) != napi_ok || key == nullptr) return;
  bool deleted = false;
  (void)napi_delete_property(env, obj, key, &deleted);
}

bool ValueToUtf8(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

bool ValueToPathString(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;
  if (ValueToUtf8(env, value, out)) return true;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t length = 0;
    if (napi_get_buffer_info(env, value, &data, &length) != napi_ok || data == nullptr) return false;
    *out = std::string(static_cast<const char*>(data), length);
    return true;
  }

  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) == napi_ok && is_typed_array) {
    napi_typedarray_type type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, value, &type, &length, &data, &arraybuffer, &byte_offset) != napi_ok ||
        data == nullptr) {
      return false;
    }
    if (type != napi_uint8_array && type != napi_uint8_clamped_array) return false;
    *out = std::string(static_cast<const char*>(data), length);
    return true;
  }

  return false;
}

size_t TypedArrayElementSize(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
    case napi_float16_array:
      return 2;
    case napi_int32_array:
    case napi_uint32_array:
    case napi_float32_array:
      return 4;
    case napi_float64_array:
    case napi_bigint64_array:
    case napi_biguint64_array:
      return 8;
    default:
      return 1;
  }
}

bool IsBufferEncoding(napi_env env, napi_value value) {
  std::string encoding;
  return ValueToUtf8(env, value, &encoding) && encoding == "buffer";
}

bool CaptureRawMethod(napi_env env, FsBindingState* st, napi_value binding, const char* name) {
  if (st == nullptr || binding == nullptr || name == nullptr) return false;
  if (st->raw_methods.find(name) != st->raw_methods.end()) return true;
  napi_value fn = nullptr;
  if (napi_get_named_property(env, binding, name, &fn) != napi_ok || fn == nullptr) return false;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, fn, &t) != napi_ok || t != napi_function) return false;
  napi_ref ref = nullptr;
  if (napi_create_reference(env, fn, 1, &ref) != napi_ok || ref == nullptr) return false;
  st->raw_methods.emplace(name, ref);
  return true;
}

napi_value NormalizeThrownUvError(napi_env env, napi_value error);

bool CallRaw(napi_env env,
             const char* method,
             size_t argc,
             napi_value* argv,
             napi_value* out,
             napi_value* error = nullptr) {
  FsBindingState* st = GetState(env);
  napi_value binding = GetBinding(env);
  if (out != nullptr) *out = nullptr;
  if (error != nullptr) *error = nullptr;
  if (st == nullptr || binding == nullptr || method == nullptr) return false;
  auto it = st->raw_methods.find(method);
  if (it == st->raw_methods.end()) return false;
  napi_value fn = GetRefValue(env, it->second);
  if (fn == nullptr) return false;
  if (napi_call_function(env, binding, fn, argc, argv, out) == napi_ok && (out == nullptr || *out != nullptr)) {
    return true;
  }
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value err = nullptr;
    napi_get_and_clear_last_exception(env, &err);
    if (error != nullptr) *error = NormalizeThrownUvError(env, err);
  }
  return false;
}

bool ErrorCodeEquals(napi_env env, napi_value error, const char* code) {
  if (error == nullptr || code == nullptr) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, error, "code", &value) != napi_ok || value == nullptr) return false;
  std::string text;
  return ValueToUtf8(env, value, &text) && text == code;
}

napi_value NormalizeThrownUvError(napi_env env, napi_value error) {
  if (error == nullptr || IsUndefined(env, error)) return error;

  bool has_errno = false;
  bool has_code = false;
  if (napi_has_named_property(env, error, "errno", &has_errno) != napi_ok || !has_errno ||
      napi_has_named_property(env, error, "code", &has_code) != napi_ok || !has_code) {
    return error;
  }

  napi_value errno_value = nullptr;
  napi_value code_value = nullptr;
  napi_value syscall_value = nullptr;
  napi_value path_value = nullptr;
  napi_value dest_value = nullptr;

  napi_get_named_property(env, error, "errno", &errno_value);
  napi_get_named_property(env, error, "code", &code_value);

  bool has_syscall = false;
  bool has_path = false;
  bool has_dest = false;
  if (napi_has_named_property(env, error, "syscall", &has_syscall) == napi_ok && has_syscall) {
    napi_get_named_property(env, error, "syscall", &syscall_value);
  }
  if (napi_has_named_property(env, error, "path", &has_path) == napi_ok && has_path) {
    napi_get_named_property(env, error, "path", &path_value);
  }
  if (napi_has_named_property(env, error, "dest", &has_dest) == napi_ok && has_dest) {
    napi_get_named_property(env, error, "dest", &dest_value);
  }

  auto delete_named = [&](const char* key) {
    napi_value name = nullptr;
    if (napi_create_string_utf8(env, key, NAPI_AUTO_LENGTH, &name) == napi_ok && name != nullptr) {
      bool ignored = false;
      napi_delete_property(env, error, name, &ignored);
    }
  };

  delete_named("errno");
  delete_named("code");
  delete_named("syscall");
  delete_named("path");
  delete_named("dest");

  if (errno_value != nullptr) napi_set_named_property(env, error, "errno", errno_value);
  if (code_value != nullptr) napi_set_named_property(env, error, "code", code_value);
  if (syscall_value != nullptr) napi_set_named_property(env, error, "syscall", syscall_value);
  if (path_value != nullptr) napi_set_named_property(env, error, "path", path_value);
  if (dest_value != nullptr) napi_set_named_property(env, error, "dest", dest_value);
  return error;
}

napi_value CreateTypedStatsArray(napi_env env, size_t length, bool as_bigint, napi_value source) {
  napi_value ab = nullptr;
  void* data = nullptr;
  const size_t byte_length = (as_bigint ? sizeof(int64_t) : sizeof(double)) * length;
  if (napi_create_arraybuffer(env, byte_length, &data, &ab) != napi_ok || ab == nullptr || data == nullptr) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  const napi_typedarray_type type = as_bigint ? napi_bigint64_array : napi_float64_array;
  if (napi_create_typedarray(env, type, length, ab, 0, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }

  if (as_bigint) {
    auto* values = static_cast<int64_t*>(data);
    for (size_t i = 0; i < length; ++i) values[i] = 0;
    if (source != nullptr) {
      for (size_t i = 0; i < length; ++i) {
        napi_value item = nullptr;
        if (napi_get_element(env, source, static_cast<uint32_t>(i), &item) != napi_ok || item == nullptr) continue;
        int64_t int64_value = 0;
        bool lossless = false;
        if (napi_get_value_bigint_int64(env, item, &int64_value, &lossless) == napi_ok) {
          values[i] = int64_value;
          continue;
        }
        double number = 0;
        if (napi_get_value_double(env, item, &number) == napi_ok) values[i] = static_cast<int64_t>(number);
      }
    }
  } else {
    auto* values = static_cast<double*>(data);
    for (size_t i = 0; i < length; ++i) values[i] = 0;
    if (source != nullptr) {
      for (size_t i = 0; i < length; ++i) {
        napi_value item = nullptr;
        if (napi_get_element(env, source, static_cast<uint32_t>(i), &item) != napi_ok || item == nullptr) continue;
        double number = 0;
        if (napi_get_value_double(env, item, &number) == napi_ok) values[i] = number;
      }
    }
  }

  return out;
}

void PopulateStatsArrayFromUv(const uv_stat_t* stat, double* out) {
  if (stat == nullptr || out == nullptr) return;
  out[0] = static_cast<double>(stat->st_dev);
  out[1] = static_cast<double>(stat->st_mode);
  out[2] = static_cast<double>(stat->st_nlink);
  out[3] = static_cast<double>(stat->st_uid);
  out[4] = static_cast<double>(stat->st_gid);
  out[5] = static_cast<double>(stat->st_rdev);
  out[6] = static_cast<double>(stat->st_blksize);
  out[7] = static_cast<double>(stat->st_ino);
  out[8] = static_cast<double>(stat->st_size);
  out[9] = static_cast<double>(stat->st_blocks);
  out[10] = static_cast<double>(stat->st_atim.tv_sec);
  out[11] = static_cast<double>(stat->st_atim.tv_nsec);
  out[12] = static_cast<double>(stat->st_mtim.tv_sec);
  out[13] = static_cast<double>(stat->st_mtim.tv_nsec);
  out[14] = static_cast<double>(stat->st_ctim.tv_sec);
  out[15] = static_cast<double>(stat->st_ctim.tv_nsec);
  out[16] = static_cast<double>(stat->st_birthtim.tv_sec);
  out[17] = static_cast<double>(stat->st_birthtim.tv_nsec);
}

napi_value CreateStatWatcherArray(napi_env env, bool as_bigint, const uv_stat_t* curr, const uv_stat_t* prev) {
  const size_t total_length = kFsStatsLength * 2;
  napi_value ab = nullptr;
  void* data = nullptr;
  const size_t byte_length = (as_bigint ? sizeof(int64_t) : sizeof(double)) * total_length;
  if (napi_create_arraybuffer(env, byte_length, &data, &ab) != napi_ok || ab == nullptr || data == nullptr) {
    return Undefined(env);
  }

  napi_value out = nullptr;
  const napi_typedarray_type type = as_bigint ? napi_bigint64_array : napi_float64_array;
  if (napi_create_typedarray(env, type, total_length, ab, 0, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }

  double curr_values[kFsStatsLength] = {};
  double prev_values[kFsStatsLength] = {};
  PopulateStatsArrayFromUv(curr, curr_values);
  PopulateStatsArrayFromUv(prev, prev_values);

  if (as_bigint) {
    auto* values = static_cast<int64_t*>(data);
    for (size_t i = 0; i < kFsStatsLength; ++i) {
      values[i] = static_cast<int64_t>(curr_values[i]);
      values[i + kFsStatsLength] = static_cast<int64_t>(prev_values[i]);
    }
  } else {
    auto* values = static_cast<double*>(data);
    for (size_t i = 0; i < kFsStatsLength; ++i) {
      values[i] = curr_values[i];
      values[i + kFsStatsLength] = prev_values[i];
    }
  }

  return out;
}

napi_value GetUsePromisesSymbol(napi_env env) {
  FsBindingState* st = GetState(env);
  if (st == nullptr) return nullptr;
  napi_value symbol = GetRefValue(env, st->k_use_promises_symbol_ref);
  if (symbol != nullptr) return symbol;
  napi_value description = nullptr;
  napi_create_string_utf8(env, "fs_use_promises_symbol", NAPI_AUTO_LENGTH, &description);
  napi_create_symbol(env, description, &symbol);
  if (symbol != nullptr) napi_create_reference(env, symbol, 1, &st->k_use_promises_symbol_ref);
  return symbol;
}

napi_value BufferFromValue(napi_env env, napi_value value, const char* encoding) {
  bool is_buffer = false;
  if (value != nullptr && napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) return value;

  napi_value global = GetGlobal(env);
  napi_value buffer_ctor = nullptr;
  if (global != nullptr &&
      napi_get_named_property(env, global, "Buffer", &buffer_ctor) == napi_ok &&
      buffer_ctor != nullptr &&
      !IsUndefined(env, buffer_ctor)) {
    // Use global Buffer.
  } else {
    napi_value require_fn = EdgeGetRequireFunction(env);
    napi_valuetype require_type = napi_undefined;
    if ((global == nullptr && require_fn == nullptr) ||
        ((require_fn == nullptr || IsUndefined(env, require_fn)) &&
         napi_get_named_property(env, global, "require", &require_fn) != napi_ok) ||
        require_fn == nullptr ||
        napi_typeof(env, require_fn, &require_type) != napi_ok ||
        require_type != napi_function) {
      return Undefined(env);
    }
    napi_value module_name = nullptr;
    napi_create_string_utf8(env, "buffer", NAPI_AUTO_LENGTH, &module_name);
    napi_value buffer_module = nullptr;
    if (napi_call_function(env, global, require_fn, 1, &module_name, &buffer_module) != napi_ok ||
        buffer_module == nullptr ||
        napi_get_named_property(env, buffer_module, "Buffer", &buffer_ctor) != napi_ok ||
        buffer_ctor == nullptr) {
      return Undefined(env);
    }
  }
  napi_value from_fn = nullptr;
  napi_valuetype t = napi_undefined;
  if (napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok ||
      from_fn == nullptr ||
      napi_typeof(env, from_fn, &t) != napi_ok ||
      t != napi_function) {
    return Undefined(env);
  }
  napi_value argv[2] = {value != nullptr ? value : Undefined(env), nullptr};
  size_t argc = 1;
  if (encoding != nullptr) {
    napi_create_string_utf8(env, encoding, NAPI_AUTO_LENGTH, &argv[1]);
    if (argv[1] != nullptr) argc = 2;
  }
  napi_value out = nullptr;
  if (napi_call_function(env, buffer_ctor, from_fn, argc, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return Undefined(env);
  }
  return out;
}

size_t ByteLengthOfValue(napi_env env, napi_value value) {
  if (value == nullptr) return 0;
  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, value, &data, &len) == napi_ok) return len;
  }
  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) == napi_ok && is_typed_array) {
    napi_typedarray_type type = napi_uint8_array;
    size_t len = 0;
    void* data = nullptr;
    napi_value ab = nullptr;
    size_t offset = 0;
    if (napi_get_typedarray_info(env, value, &type, &len, &data, &ab, &offset) == napi_ok) {
      return len * TypedArrayElementSize(type);
    }
  }
  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t len = 0;
    void* data = nullptr;
    napi_value ab = nullptr;
    size_t offset = 0;
    if (napi_get_dataview_info(env, value, &len, &data, &ab, &offset) == napi_ok) return len;
  }
  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_arraybuffer_info(env, value, &data, &len) == napi_ok) return len;
  }
  return 0;
}

napi_value ConvertNameArrayToEncoding(napi_env env, napi_value names, napi_value encoding_value) {
  napi_valuetype encoding_type = napi_undefined;
  if (names == nullptr ||
      IsUndefined(env, names) ||
      encoding_value == nullptr ||
      napi_typeof(env, encoding_value, &encoding_type) != napi_ok ||
      encoding_type == napi_undefined ||
      encoding_type == napi_null) {
    return names;
  }
  std::string encoding;
  if (!ValueToUtf8(env, encoding_value, &encoding) || encoding.empty() || encoding == "utf8" || encoding == "utf-8") {
    return names;
  }
  uint32_t len = 0;
  bool is_array = false;
  if (napi_is_array(env, names, &is_array) != napi_ok || !is_array ||
      napi_get_array_length(env, names, &len) != napi_ok) {
    return names;
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, len, &out) != napi_ok || out == nullptr) return names;
  for (uint32_t i = 0; i < len; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, names, i, &item) != napi_ok || item == nullptr) continue;
    napi_value converted = item;
    if (encoding == "buffer") {
      converted = BufferFromValue(env, item, "utf8");
    } else {
      napi_value buffer = BufferFromValue(env, item, "utf8");
      napi_value to_string = nullptr;
      napi_value encoding_arg = nullptr;
      if (buffer != nullptr &&
          !IsUndefined(env, buffer) &&
          napi_get_named_property(env, buffer, "toString", &to_string) == napi_ok &&
          to_string != nullptr &&
          napi_create_string_utf8(env, encoding.c_str(), encoding.size(), &encoding_arg) == napi_ok &&
          encoding_arg != nullptr) {
        napi_value argv[1] = {encoding_arg};
        napi_value encoded = nullptr;
        if (napi_call_function(env, buffer, to_string, 1, argv, &encoded) == napi_ok && encoded != nullptr) {
          converted = encoded;
        }
      }
    }
    if (converted == nullptr || IsUndefined(env, converted)) converted = item;
    napi_set_element(env, out, i, converted);
  }
  return out;
}

napi_value MakeResolvedPromise(napi_env env, napi_value value) {
  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) return Undefined(env);
  napi_resolve_deferred(env, deferred, value != nullptr ? value : Undefined(env));
  return promise;
}

napi_value MakeRejectedPromise(napi_env env, napi_value reason) {
  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) return Undefined(env);
  napi_reject_deferred(env, deferred, reason != nullptr ? reason : Undefined(env));
  return promise;
}

enum class ReqKind {
  kNone,
  kPromise,
  kCallback,
};

struct DeferredReqCompletion {
  napi_env env = nullptr;
  napi_ref req_ref = nullptr;
  napi_ref oncomplete_ref = nullptr;
  napi_ref err_ref = nullptr;
  napi_ref value_ref = nullptr;
  napi_ref extra_ref = nullptr;
};

struct DeferredPromiseSettlement {
  napi_env env = nullptr;
  napi_deferred deferred = nullptr;
  napi_ref value_ref = nullptr;
  napi_ref err_ref = nullptr;
  bool reject = false;
};

struct FsReqCallbackWrap {
  napi_env env = nullptr;
  int64_t async_id = 0;
  bool destroy_queued = false;
};

void TrackActiveRequest(napi_env env, napi_value req) {
  if (req == nullptr || IsUndefined(env, req)) return;
  EdgeTrackActiveRequest(env, req, "FSReqCallback");
}

void UntrackActiveRequest(napi_env env, napi_value req) {
  if (req == nullptr || IsUndefined(env, req)) return;
  EdgeUntrackActiveRequest(env, req);
}

FsReqCallbackWrap* UnwrapFsReqCallback(napi_env env, napi_value req) {
  if (env == nullptr || req == nullptr || IsUndefined(env, req)) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, req, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<FsReqCallbackWrap*>(data);
}

void QueueFsReqCallbackDestroy(FsReqCallbackWrap* wrap) {
  if (wrap == nullptr || wrap->destroy_queued || wrap->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(wrap->env, wrap->async_id);
  wrap->destroy_queued = true;
}

void FSReqCallbackFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<FsReqCallbackWrap*>(data);
  if (wrap == nullptr) return;
  if (env != nullptr) {
    wrap->env = env;
  }
  QueueFsReqCallbackDestroy(wrap);
  delete wrap;
}

void InvokeFsReqCallback(napi_env env,
                         napi_value req,
                         napi_value oncomplete,
                         size_t argc,
                         napi_value* argv) {
  if (env == nullptr || req == nullptr || oncomplete == nullptr) return;
  FsReqCallbackWrap* wrap = UnwrapFsReqCallback(env, req);
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(env,
                                 wrap != nullptr ? wrap->async_id : -1,
                                 req,
                                 req,
                                 oncomplete,
                                 argc,
                                 argv,
                                 &ignored,
                                 kEdgeMakeCallbackNone);
  QueueFsReqCallbackDestroy(wrap);
}

void DestroyDeferredReqCompletion(DeferredReqCompletion* completion) {
  if (completion == nullptr) return;
  napi_env env = completion->env;
  if (env != nullptr) {
    ResetRef(env, &completion->req_ref);
    ResetRef(env, &completion->oncomplete_ref);
    ResetRef(env, &completion->err_ref);
    ResetRef(env, &completion->value_ref);
    ResetRef(env, &completion->extra_ref);
  }
  delete completion;
}

void DestroyDeferredPromiseSettlement(DeferredPromiseSettlement* settlement) {
  if (settlement == nullptr) return;
  napi_env env = settlement->env;
  if (env != nullptr) {
    ResetRef(env, &settlement->value_ref);
    ResetRef(env, &settlement->err_ref);
  }
  delete settlement;
}

void InvokeDeferredReqCompletion(napi_env env, DeferredReqCompletion* completion) {
  if (completion == nullptr || env == nullptr) return;
  if (EdgeWorkerEnvStopRequested(env)) {
    DestroyDeferredReqCompletion(completion);
    return;
  }
  napi_value req = GetRefValue(env, completion->req_ref);
  napi_value oncomplete = GetRefValue(env, completion->oncomplete_ref);
  napi_value err = GetRefValue(env, completion->err_ref);
  napi_value value = GetRefValue(env, completion->value_ref);
  napi_value extra = GetRefValue(env, completion->extra_ref);
  if (req != nullptr) UntrackActiveRequest(env, req);

  if (req == nullptr || oncomplete == nullptr) {
    DestroyDeferredReqCompletion(completion);
    return;
  }

  if (err != nullptr && !IsUndefined(env, err)) {
    napi_value argv[1] = {err};
    InvokeFsReqCallback(env, req, oncomplete, 1, argv);
  } else if (value != nullptr && !IsUndefined(env, value)) {
    napi_value null_value = nullptr;
    napi_get_null(env, &null_value);
    napi_value argv[3] = {null_value != nullptr ? null_value : Undefined(env), value, extra};
    const size_t argc = (extra != nullptr && !IsUndefined(env, extra)) ? 3 : 2;
    InvokeFsReqCallback(env, req, oncomplete, argc, argv);
  } else {
    napi_value null_value = nullptr;
    napi_get_null(env, &null_value);
    napi_value argv[1] = {null_value != nullptr ? null_value : Undefined(env)};
    InvokeFsReqCallback(env, req, oncomplete, 1, argv);
  }
  DestroyDeferredReqCompletion(completion);
}

napi_value DeferredReqCompletionCallback(napi_env env, napi_callback_info info) {
  void* data = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, nullptr, &data);
  auto* completion = static_cast<DeferredReqCompletion*>(data);
  InvokeDeferredReqCompletion(env, completion);
  return Undefined(env);
}

napi_value DeferredPromiseSettlementCallback(napi_env env, napi_callback_info info) {
  void* data = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, nullptr, &data);
  auto* settlement = static_cast<DeferredPromiseSettlement*>(data);
  if (settlement == nullptr || env == nullptr) return Undefined(env);
  if (EdgeWorkerEnvStopRequested(env)) {
    DestroyDeferredPromiseSettlement(settlement);
    return Undefined(env);
  }

  napi_value value = GetRefValue(env, settlement->value_ref);
  napi_value err = GetRefValue(env, settlement->err_ref);
  if (settlement->deferred != nullptr) {
    if (settlement->reject) {
      (void)napi_reject_deferred(env, settlement->deferred, err != nullptr ? err : Undefined(env));
    } else {
      (void)napi_resolve_deferred(env, settlement->deferred, value != nullptr ? value : Undefined(env));
    }
  }
  DestroyDeferredPromiseSettlement(settlement);
  return Undefined(env);
}

bool ScheduleDeferredReqCompletion(napi_env env,
                                   napi_value req,
                                   napi_value oncomplete,
                                   napi_value err,
                                   napi_value value,
                                   napi_value extra = nullptr,
                                   bool use_set_immediate = false) {
  if (env == nullptr || req == nullptr || oncomplete == nullptr) return false;
  auto* completion = new DeferredReqCompletion();
  completion->env = env;
  if (napi_create_reference(env, req, 1, &completion->req_ref) != napi_ok ||
      napi_create_reference(env, oncomplete, 1, &completion->oncomplete_ref) != napi_ok ||
      (err != nullptr && !IsUndefined(env, err) &&
       napi_create_reference(env, err, 1, &completion->err_ref) != napi_ok) ||
      (value != nullptr && !IsUndefined(env, value) &&
       napi_create_reference(env, value, 1, &completion->value_ref) != napi_ok) ||
      (extra != nullptr && !IsUndefined(env, extra) &&
       napi_create_reference(env, extra, 1, &completion->extra_ref) != napi_ok)) {
    DestroyDeferredReqCompletion(completion);
    return false;
  }

  TrackActiveRequest(env, req);

  napi_value callback = nullptr;
  if (napi_create_function(env,
                           "__ubiFsDeferredReqComplete",
                           NAPI_AUTO_LENGTH,
                           DeferredReqCompletionCallback,
                           completion,
                           &callback) != napi_ok ||
      callback == nullptr) {
    UntrackActiveRequest(env, req);
    DestroyDeferredReqCompletion(completion);
    return false;
  }

  napi_value global = GetGlobal(env);
  napi_value scheduler_recv = global;
  napi_value scheduler = nullptr;
  napi_valuetype scheduler_type = napi_undefined;
  if (global == nullptr) {
    UntrackActiveRequest(env, req);
    DestroyDeferredReqCompletion(completion);
    return false;
  }
  if (use_set_immediate) {
    if (napi_get_named_property(env, global, "setImmediate", &scheduler) != napi_ok ||
        scheduler == nullptr ||
        napi_typeof(env, scheduler, &scheduler_type) != napi_ok ||
        scheduler_type != napi_function) {
      UntrackActiveRequest(env, req);
      DestroyDeferredReqCompletion(completion);
      return false;
    }
  } else {
    napi_value process = nullptr;
    if (napi_get_named_property(env, global, "process", &process) != napi_ok ||
        process == nullptr ||
        napi_get_named_property(env, process, "nextTick", &scheduler) != napi_ok ||
        scheduler == nullptr ||
        napi_typeof(env, scheduler, &scheduler_type) != napi_ok ||
        scheduler_type != napi_function) {
      UntrackActiveRequest(env, req);
      DestroyDeferredReqCompletion(completion);
      return false;
    }
    scheduler_recv = process;
  }

  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(env, scheduler_recv, scheduler, 1, argv, &ignored) != napi_ok) {
    UntrackActiveRequest(env, req);
    DestroyDeferredReqCompletion(completion);
    return false;
  }

  return true;
}

bool ScheduleDeferredPromiseSettlement(napi_env env,
                                      napi_deferred deferred,
                                      napi_value value,
                                      napi_value err,
                                      bool reject) {
  if (env == nullptr || deferred == nullptr) return false;
  auto* settlement = new DeferredPromiseSettlement();
  settlement->env = env;
  settlement->deferred = deferred;
  settlement->reject = reject;

  if ((value != nullptr && !IsUndefined(env, value) &&
       napi_create_reference(env, value, 1, &settlement->value_ref) != napi_ok) ||
      (err != nullptr && !IsUndefined(env, err) &&
       napi_create_reference(env, err, 1, &settlement->err_ref) != napi_ok)) {
    DestroyDeferredPromiseSettlement(settlement);
    return false;
  }

  napi_value callback = nullptr;
  if (napi_create_function(env,
                           "__ubiFsDeferredPromiseSettlement",
                           NAPI_AUTO_LENGTH,
                           DeferredPromiseSettlementCallback,
                           settlement,
                           &callback) != napi_ok ||
      callback == nullptr) {
    DestroyDeferredPromiseSettlement(settlement);
    return false;
  }

  napi_value global = GetGlobal(env);
  napi_value set_immediate = nullptr;
  napi_valuetype set_immediate_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(env, global, "setImmediate", &set_immediate) != napi_ok ||
      set_immediate == nullptr ||
      napi_typeof(env, set_immediate, &set_immediate_type) != napi_ok ||
      set_immediate_type != napi_function) {
    DestroyDeferredPromiseSettlement(settlement);
    return false;
  }

  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(env, global, set_immediate, 1, argv, &ignored) != napi_ok) {
    DestroyDeferredPromiseSettlement(settlement);
    return false;
  }

  return true;
}

napi_value MakeDeferredResolvedPromise(napi_env env, napi_value value) {
  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) {
    return Undefined(env);
  }
  if (!ScheduleDeferredPromiseSettlement(env, deferred, value, nullptr, false)) {
    napi_resolve_deferred(env, deferred, value != nullptr ? value : Undefined(env));
  }
  return promise;
}

napi_value MakeDeferredRejectedPromise(napi_env env, napi_value err) {
  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) {
    return Undefined(env);
  }
  if (!ScheduleDeferredPromiseSettlement(env, deferred, nullptr, err, true)) {
    napi_reject_deferred(env, deferred, err != nullptr ? err : Undefined(env));
  }
  return promise;
}

ReqKind ParseReq(napi_env env, napi_value candidate, napi_value* oncomplete) {
  if (oncomplete != nullptr) *oncomplete = nullptr;
  if (candidate == nullptr || IsUndefined(env, candidate)) return ReqKind::kNone;

  napi_value symbol = GetUsePromisesSymbol(env);
  if (symbol != nullptr) {
    bool same = false;
    if (napi_strict_equals(env, candidate, symbol, &same) == napi_ok && same) return ReqKind::kPromise;
  }

  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, candidate, &t) == napi_ok && (t == napi_object || t == napi_external)) {
    napi_value fn = nullptr;
    napi_valuetype fn_t = napi_undefined;
    if (napi_get_named_property(env, candidate, "oncomplete", &fn) == napi_ok &&
        fn != nullptr &&
        napi_typeof(env, fn, &fn_t) == napi_ok &&
        fn_t == napi_function) {
      if (oncomplete != nullptr) *oncomplete = fn;
      return ReqKind::kCallback;
    }
  }
  return ReqKind::kNone;
}

void CompleteReq(napi_env env, ReqKind kind, napi_value req, napi_value oncomplete, napi_value err, napi_value value) {
  if (kind != ReqKind::kCallback || req == nullptr || oncomplete == nullptr) return;
  if (ScheduleDeferredReqCompletion(env, req, oncomplete, err, value)) return;
  if (err != nullptr && !IsUndefined(env, err)) {
    napi_value argv[1] = {err};
    InvokeFsReqCallback(env, req, oncomplete, 1, argv);
    return;
  }
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  if (value != nullptr && !IsUndefined(env, value)) {
    napi_value argv[2] = {null_value != nullptr ? null_value : Undefined(env), value};
    InvokeFsReqCallback(env, req, oncomplete, 2, argv);
    return;
  }
  napi_value argv[1] = {null_value != nullptr ? null_value : Undefined(env)};
  InvokeFsReqCallback(env, req, oncomplete, 1, argv);
}

void CompleteReqWithExtra(napi_env env,
                          ReqKind kind,
                          napi_value req,
                          napi_value oncomplete,
                          napi_value err,
                          napi_value value,
                          napi_value extra) {
  if (kind != ReqKind::kCallback || req == nullptr || oncomplete == nullptr) return;
  if (ScheduleDeferredReqCompletion(env, req, oncomplete, err, value, extra)) return;
  if (err != nullptr && !IsUndefined(env, err)) {
    napi_value argv[1] = {err};
    InvokeFsReqCallback(env, req, oncomplete, 1, argv);
    return;
  }
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  if (value != nullptr && !IsUndefined(env, value)) {
    napi_value argv[3] = {null_value != nullptr ? null_value : Undefined(env), value, extra};
    const size_t argc = (extra != nullptr && !IsUndefined(env, extra)) ? 3 : 2;
    InvokeFsReqCallback(env, req, oncomplete, argc, argv);
    return;
  }
  napi_value argv[1] = {null_value != nullptr ? null_value : Undefined(env)};
  InvokeFsReqCallback(env, req, oncomplete, 1, argv);
}

napi_value CompleteVoidRawFsMethod(napi_env env,
                                   const char* method,
                                   ReqKind req_kind,
                                   napi_value req,
                                   napi_value oncomplete,
                                   size_t argc,
                                   napi_value* argv) {
  napi_value out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, method, argc, argv, &out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

struct FileHandleReadReq;

struct FileHandleWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
  napi_ref closing_promise_ref = nullptr;
  napi_deferred closing_deferred = nullptr;
  int32_t fd = -1;
  int64_t read_offset = -1;
  int64_t read_length = -1;
  FileHandleReadReq* current_read = nullptr;
  bool reading = false;
  bool closing = false;
  bool closed = false;
};

struct FileHandleReadReq {
  uv_fs_t req{};
  FileHandleWrap* wrap = nullptr;
  void* active_request_token = nullptr;
  char* storage = nullptr;
  uv_buf_t buf{};
};

struct FileHandleCloseReq {
  napi_env env = nullptr;
  FileHandleWrap* wrap = nullptr;
  uv_fs_t req{};
  napi_ref req_ref = nullptr;
  void* active_request_token = nullptr;
  bool is_shutdown = false;
};

enum class AsyncFsResultKind {
  kUndefined,
  kInt64,
  kFileHandle,
};

struct AsyncFsReq {
  napi_env env = nullptr;
  ReqKind req_kind = ReqKind::kNone;
  napi_ref req_ref = nullptr;
  napi_ref oncomplete_ref = nullptr;
  napi_ref extra_ref = nullptr;
  void* active_request_token = nullptr;
  napi_deferred deferred = nullptr;
  uv_fs_t req{};
  AsyncFsResultKind result_kind = AsyncFsResultKind::kUndefined;
  const char* syscall = nullptr;
  std::string path_storage;
  napi_ref* hold_refs = nullptr;
  size_t hold_ref_count = 0;
  uv_buf_t* bufs = nullptr;
  size_t nbufs = 0;
  bool track_unmanaged_fd = false;
  bool uses_uv_fs_req = false;
  int32_t open_flags = 0;
  bool is_open_req = false;
  bool delayed_open_completion = false;
  bool pending_fifo_writer_open = false;
  struct DeferredOpenCompletionTask* deferred_open_task = nullptr;
};

struct DeferredOpenCompletionTask {
  napi_env env = nullptr;
  AsyncFsReq* async_req = nullptr;
};

FsBindingState::~FsBindingState() {
  for (auto& entry : pending_fifo_writer_opens) {
    if (entry.second == nullptr) continue;
    entry.second->pending_fifo_writer_open = false;
    if (entry.second->deferred_open_task != nullptr) {
      entry.second->deferred_open_task->async_req = nullptr;
      entry.second->deferred_open_task = nullptr;
    }
    DestroyAsyncFsReq(entry.second);
  }
  pending_fifo_writer_opens.clear();
  for (auto& entry : raw_methods) {
    ResetRef(env, &entry.second);
  }
  raw_methods.clear();
  ResetRef(env, &binding_ref);
  ResetRef(env, &file_handle_ctor_ref);
  ResetRef(env, &fs_req_ctor_ref);
  ResetRef(env, &stat_watcher_ctor_ref);
  ResetRef(env, &k_use_promises_symbol_ref);
}

void HoldFileHandleRef(FileHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->base.wrapper_ref == nullptr) return;
  uint32_t ref_count = 0;
  (void)napi_reference_ref(wrap->env, wrap->base.wrapper_ref, &ref_count);
}

void ReleaseFileHandleRef(FileHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->base.wrapper_ref == nullptr) return;
  uint32_t ref_count = 0;
  (void)napi_reference_unref(wrap->env, wrap->base.wrapper_ref, &ref_count);
}

napi_value GetFileHandleOwner(napi_env env, FileHandleWrap* wrap) {
  if (env == nullptr || wrap == nullptr) return nullptr;
  return EdgeStreamBaseGetWrapper(&wrap->base);
}

napi_value GetFileHandleReadReqOwner(napi_env env, void* data) {
  auto* req = static_cast<FileHandleReadReq*>(data);
  return req != nullptr ? GetFileHandleOwner(env, req->wrap) : nullptr;
}

void CancelFileHandleReadReq(void* data) {
  auto* req = static_cast<FileHandleReadReq*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

napi_value GetFileHandleCloseReqOwner(napi_env env, void* data) {
  auto* req = static_cast<FileHandleCloseReq*>(data);
  if (req == nullptr) return nullptr;
  napi_value owner = GetRefValue(env, req->req_ref);
  return owner != nullptr ? owner : GetFileHandleOwner(env, req->wrap);
}

void CancelFileHandleCloseReq(void* data) {
  auto* req = static_cast<FileHandleCloseReq*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

napi_value CreateUvExceptionValue(napi_env env, int errorno, const char* syscall);
napi_value CreateUvExceptionValue(napi_env env, int errorno, const char* syscall, const char* path);

void DestroyAsyncFsReq(AsyncFsReq* async_req) {
  if (async_req == nullptr) return;
  napi_env env = async_req->env;
  if (env != nullptr && async_req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(env, async_req->active_request_token);
    async_req->active_request_token = nullptr;
  }
  if (env != nullptr) {
    ResetRef(env, &async_req->req_ref);
    ResetRef(env, &async_req->oncomplete_ref);
    ResetRef(env, &async_req->extra_ref);
    if (async_req->hold_refs != nullptr) {
      for (size_t i = 0; i < async_req->hold_ref_count; ++i) {
        ResetRef(env, &async_req->hold_refs[i]);
      }
    }
  }
  delete[] async_req->hold_refs;
  delete[] async_req->bufs;
  if (async_req->uses_uv_fs_req) {
    uv_fs_req_cleanup(&async_req->req);
  }
  if (async_req->deferred_open_task != nullptr) {
    async_req->deferred_open_task->async_req = nullptr;
    async_req->deferred_open_task = nullptr;
  }
  delete async_req;
}

int FsOpenAccessMode(int32_t flags) {
  return flags & (UV_FS_O_RDONLY | UV_FS_O_WRONLY | UV_FS_O_RDWR);
}

bool ShouldDelayOpenCompletion(AsyncFsReq* async_req, int result) {
  if (async_req == nullptr || result < 0 || !async_req->is_open_req) return false;
  if (FsOpenAccessMode(async_req->open_flags) != UV_FS_O_WRONLY) return false;

  uv_fs_t stat_req{};
  const int stat_result = uv_fs_fstat(nullptr, &stat_req, result, nullptr);
  bool is_fifo = false;
  if (stat_result == 0) {
    is_fifo = S_ISFIFO(stat_req.statbuf.st_mode);
  }
  uv_fs_req_cleanup(&stat_req);
  return is_fifo;
}

void RunDeferredOpenCompletion(napi_env env, void* data) {
  auto* task = static_cast<DeferredOpenCompletionTask*>(data);
  if (task == nullptr) return;
  auto* async_req = task->async_req;
  if (async_req == nullptr || env == nullptr || async_req->env != env) {
    delete task;
    return;
  }
  FsBindingState* st = GetState(env);
  if (st != nullptr && async_req->pending_fifo_writer_open) {
    auto range = st->pending_fifo_writer_opens.equal_range(async_req->path_storage);
    for (auto it = range.first; it != range.second; ++it) {
      if (it->second == async_req) {
        st->pending_fifo_writer_opens.erase(it);
        break;
      }
    }
  }
  async_req->delayed_open_completion = false;
  async_req->pending_fifo_writer_open = false;
  async_req->deferred_open_task = nullptr;
  task->async_req = nullptr;
  delete task;
  FinishAsyncFsReq(async_req, static_cast<int>(async_req->req.result));
}

napi_value DeferredOpenCompletionCallback(napi_env env, napi_callback_info info) {
  void* data = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, nullptr, &data);
  auto* async_req = static_cast<AsyncFsReq*>(data);
  RunDeferredOpenCompletion(env, async_req);
  return Undefined(env);
}

bool ScheduleDeferredOpenCompletion(AsyncFsReq* async_req) {
  if (async_req == nullptr || async_req->env == nullptr) return false;
  napi_env env = async_req->env;
  auto* task = new DeferredOpenCompletionTask();
  task->env = env;
  task->async_req = async_req;
  napi_value callback = nullptr;
  if (napi_create_function(env,
                           "__ubiFsDeferredOpenComplete",
                           NAPI_AUTO_LENGTH,
                           DeferredOpenCompletionCallback,
                           task,
                           &callback) != napi_ok ||
      callback == nullptr) {
    delete task;
    return false;
  }
  napi_value global = GetGlobal(env);
  napi_value set_timeout = nullptr;
  napi_valuetype set_timeout_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(env, global, "setTimeout", &set_timeout) != napi_ok ||
      set_timeout == nullptr ||
      napi_typeof(env, set_timeout, &set_timeout_type) != napi_ok ||
      set_timeout_type != napi_function) {
    delete task;
    return false;
  }
  napi_value delay = nullptr;
  napi_create_int32(env, 50, &delay);
  napi_value argv[2] = {callback, delay != nullptr ? delay : Undefined(env)};
  napi_value ignored = nullptr;
  if (napi_call_function(env, global, set_timeout, 2, argv, &ignored) != napi_ok) {
    delete task;
    return false;
  }
  async_req->deferred_open_task = task;
  return true;
}

void ReleasePendingFifoWriterOpens(napi_env env, const std::string& path) {
  FsBindingState* st = GetState(env);
  if (st == nullptr || path.empty()) return;

  std::vector<AsyncFsReq*> pending;
  auto range = st->pending_fifo_writer_opens.equal_range(path);
  for (auto it = range.first; it != range.second; ++it) {
    if (it->second != nullptr) pending.push_back(it->second);
  }
  st->pending_fifo_writer_opens.erase(range.first, range.second);

  for (AsyncFsReq* async_req : pending) {
    if (async_req == nullptr) continue;
    async_req->pending_fifo_writer_open = false;
    async_req->delayed_open_completion = false;
    if (async_req->deferred_open_task != nullptr) {
      async_req->deferred_open_task->async_req = nullptr;
      async_req->deferred_open_task = nullptr;
    }
    FinishAsyncFsReq(async_req, static_cast<int>(async_req->req.result));
  }
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

void ThrowInvalidFdType(napi_env env) {
  napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"fd\" argument must be of type number");
}

void ThrowFdOutOfRange(napi_env env, int32_t value) {
  std::string message =
      "The value of \"fd\" is out of range. It must be >= 0 && <= 2147483647. Received ";
  message += std::to_string(value);
  napi_throw_range_error(env, "ERR_OUT_OF_RANGE", message.c_str());
}

std::string ValueToDisplayString(napi_env env, napi_value value) {
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

std::string DescribeReceivedValue(napi_env env, napi_value value) {
  if (value == nullptr) return "undefined";
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return "undefined";

  switch (type) {
    case napi_undefined:
      return "undefined";
    case napi_null:
      return "null";
    case napi_boolean:
      return "type boolean (" + ValueToDisplayString(env, value) + ")";
    case napi_string:
      return "type string ('" + ValueToDisplayString(env, value) + "')";
    case napi_number:
      return "type number (" + ValueToDisplayString(env, value) + ")";
    case napi_object: {
      bool is_array = false;
      if (napi_is_array(env, value, &is_array) == napi_ok && is_array) {
        return "an instance of Array";
      }
      return "an instance of Object";
    }
    case napi_function:
      return "an instance of Function";
    case napi_symbol:
      return "type symbol";
    case napi_bigint:
      return "type bigint (" + ValueToDisplayString(env, value) + ")";
    default:
      return ValueToDisplayString(env, value);
  }
}

void ThrowInvalidNumberArgType(napi_env env, const char* name, napi_value value) {
  std::string message = "The \"";
  message += (name != nullptr ? name : "value");
  message += "\" argument must be of type number. Received ";
  message += DescribeReceivedValue(env, value);
  napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", message.c_str());
}

void ThrowOutOfRangeIntegerArg(napi_env env, const char* name, napi_value value) {
  std::string message = "The value of \"";
  message += (name != nullptr ? name : "value");
  message += "\" is out of range. It must be an integer. Received ";
  message += ValueToDisplayString(env, value);
  napi_throw_range_error(env, "ERR_OUT_OF_RANGE", message.c_str());
}

void ThrowOutOfRangeBoundedArg(napi_env env,
                               const char* name,
                               int64_t min_value,
                               uint64_t max_value,
                               napi_value value) {
  std::ostringstream message;
  message << "The value of \"" << (name != nullptr ? name : "value")
          << "\" is out of range. It must be >= " << min_value << " && <= " << max_value
          << ". Received " << ValueToDisplayString(env, value);
  napi_throw_range_error(env, "ERR_OUT_OF_RANGE", message.str().c_str());
}

bool ValidateFdArg(napi_env env, napi_value value, int32_t* fd_out) {
  if (fd_out == nullptr) return false;
  *fd_out = -1;
  if (value == nullptr) {
    ThrowInvalidNumberArgType(env, "fd", value);
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) {
    ThrowInvalidNumberArgType(env, "fd", value);
    return false;
  }
  double number = 0;
  if (napi_get_value_double(env, value, &number) != napi_ok) {
    ThrowInvalidNumberArgType(env, "fd", value);
    return false;
  }
  if (!std::isfinite(number) || std::floor(number) != number) {
    ThrowOutOfRangeIntegerArg(env, "fd", value);
    return false;
  }
  if (number < 0 || number > static_cast<double>(std::numeric_limits<int32_t>::max())) {
    ThrowOutOfRangeBoundedArg(env, "fd", 0, static_cast<uint64_t>(std::numeric_limits<int32_t>::max()), value);
    return false;
  }
  *fd_out = static_cast<int32_t>(number);
  return true;
}

bool ValidateCopyFileModeArg(napi_env env, napi_value value, int32_t* mode_out) {
  if (mode_out == nullptr) return false;
  *mode_out = 0;
  if (value == nullptr) {
    ThrowInvalidNumberArgType(env, "mode", value);
    return false;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) {
    ThrowInvalidNumberArgType(env, "mode", value);
    return false;
  }
  double number = 0;
  if (napi_get_value_double(env, value, &number) != napi_ok) {
    ThrowInvalidNumberArgType(env, "mode", value);
    return false;
  }
  if (!std::isfinite(number) || std::floor(number) != number) {
    ThrowOutOfRangeIntegerArg(env, "mode", value);
    return false;
  }
  constexpr uint64_t kMaxCopyFileMode =
      UV_FS_COPYFILE_EXCL | UV_FS_COPYFILE_FICLONE | UV_FS_COPYFILE_FICLONE_FORCE;
  if (number < 0 || number > static_cast<double>(kMaxCopyFileMode)) {
    ThrowOutOfRangeBoundedArg(env, "mode", 0, kMaxCopyFileMode, value);
    return false;
  }
  *mode_out = static_cast<int32_t>(number);
  return true;
}

bool ValidateAccessModeArg(napi_env env, napi_value value, int32_t* mode_out) {
  if (mode_out == nullptr) return false;
  *mode_out = F_OK;
  if (value == nullptr || IsNullOrUndefined(env, value)) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) {
    ThrowInvalidNumberArgType(env, "mode", value);
    return false;
  }
  double number = 0;
  if (napi_get_value_double(env, value, &number) != napi_ok) {
    ThrowInvalidNumberArgType(env, "mode", value);
    return false;
  }
  if (!std::isfinite(number) || std::floor(number) != number) {
    ThrowOutOfRangeIntegerArg(env, "mode", value);
    return false;
  }
  constexpr uint64_t kMaxAccessMode = F_OK | R_OK | W_OK | X_OK;
  if (number < 0 || number > static_cast<double>(kMaxAccessMode)) {
    ThrowOutOfRangeBoundedArg(env, "mode", 0, kMaxAccessMode, value);
    return false;
  }
  *mode_out = static_cast<int32_t>(number);
  return true;
}

int64_t GetInt64OrDefault(napi_env env, napi_value value, int64_t fallback) {
  if (IsNullOrUndefined(env, value)) return fallback;
  int64_t out = fallback;
  if (napi_get_value_int64(env, value, &out) == napi_ok) return out;
  bool lossless = false;
  if (napi_get_value_bigint_int64(env, value, &out, &lossless) == napi_ok) return out;
  return out;
}

bool ExtractByteSpanForAsyncIo(napi_env env,
                               napi_value value,
                               size_t offset,
                               size_t length,
                               napi_value* hold_value,
                               uv_buf_t* out) {
  if (hold_value == nullptr || out == nullptr) return false;

  napi_value backing = value;
  void* raw = nullptr;
  size_t total_len = 0;

  bool is_buffer = false;
  if (backing != nullptr && napi_is_buffer(env, backing, &is_buffer) == napi_ok && is_buffer) {
    if (napi_get_buffer_info(env, backing, &raw, &total_len) != napi_ok) return false;
  } else {
    bool is_typed_array = false;
    if (backing != nullptr && napi_is_typedarray(env, backing, &is_typed_array) == napi_ok && is_typed_array) {
      napi_typedarray_type type = napi_uint8_array;
      size_t len = 0;
      napi_value ab = nullptr;
      size_t byte_offset = 0;
      if (napi_get_typedarray_info(env, backing, &type, &len, &raw, &ab, &byte_offset) != napi_ok) return false;
      total_len = len * TypedArrayElementSize(type);
    } else {
      bool is_dataview = false;
      if (backing != nullptr && napi_is_dataview(env, backing, &is_dataview) == napi_ok && is_dataview) {
        napi_value ab = nullptr;
        size_t byte_offset = 0;
        if (napi_get_dataview_info(env, backing, &total_len, &raw, &ab, &byte_offset) != napi_ok) return false;
      } else {
        backing = BufferFromValue(env, value, nullptr);
        if (backing == nullptr || IsUndefined(env, backing)) return false;
        if (napi_get_buffer_info(env, backing, &raw, &total_len) != napi_ok) return false;
      }
    }
  }

  if (raw == nullptr || offset > total_len) return false;
  if (length > total_len - offset) length = total_len - offset;

  *hold_value = backing;
  *out = uv_buf_init(static_cast<char*>(raw) + offset, static_cast<unsigned int>(length));
  return true;
}

napi_value MakeAsyncFsResultValue(AsyncFsReq* async_req, int64_t result) {
  if (async_req == nullptr || async_req->env == nullptr) return nullptr;
  napi_env env = async_req->env;
  switch (async_req->result_kind) {
    case AsyncFsResultKind::kUndefined:
      return Undefined(env);
    case AsyncFsResultKind::kInt64: {
      napi_value out = nullptr;
      napi_create_int64(env, result, &out);
      return out != nullptr ? out : Undefined(env);
    }
    case AsyncFsResultKind::kFileHandle: {
      FsBindingState* st = GetState(env);
      napi_value ctor = st == nullptr ? nullptr : GetRefValue(env, st->file_handle_ctor_ref);
      if (ctor == nullptr) return Undefined(env);
      napi_value fd_value = nullptr;
      napi_create_int32(env, static_cast<int32_t>(result), &fd_value);
      napi_value argv[1] = {fd_value};
      napi_value handle = nullptr;
      if (napi_new_instance(env, ctor, 1, argv, &handle) != napi_ok || handle == nullptr) {
        return Undefined(env);
      }
      return handle;
    }
  }
  return Undefined(env);
}

void FinishAsyncFsReq(AsyncFsReq* async_req, int result) {
  if (async_req == nullptr || async_req->env == nullptr) {
    DestroyAsyncFsReq(async_req);
    return;
  }

  napi_env env = async_req->env;
  if (EdgeWorkerEnvStopRequested(env)) {
    DestroyAsyncFsReq(async_req);
    return;
  }
  napi_value err = nullptr;
  napi_value value = Undefined(env);
  if (result < 0) {
    if (!async_req->path_storage.empty()) {
      err = CreateUvExceptionValue(env,
                                   result,
                                   async_req->syscall != nullptr ? async_req->syscall : "",
                                   async_req->path_storage.c_str());
    } else {
      err = CreateUvExceptionValue(env, result, async_req->syscall != nullptr ? async_req->syscall : "");
    }
  } else {
    if (async_req->track_unmanaged_fd) {
      EdgeWorkerEnvAddUnmanagedFd(env, static_cast<int>(result));
    }
    value = MakeAsyncFsResultValue(async_req, result);
  }

  if (async_req->req_kind == ReqKind::kPromise && async_req->deferred != nullptr) {
    if (result < 0) {
      (void)napi_reject_deferred(env, async_req->deferred, err != nullptr ? err : Undefined(env));
    } else {
      (void)napi_resolve_deferred(env, async_req->deferred, value != nullptr ? value : Undefined(env));
    }
    (void)EdgeRunCallbackScopeCheckpoint(env);
    DestroyAsyncFsReq(async_req);
    return;
  }

  if (async_req->req_kind == ReqKind::kCallback) {
    napi_value req = GetRefValue(env, async_req->req_ref);
    napi_value oncomplete = GetRefValue(env, async_req->oncomplete_ref);
    napi_value extra = GetRefValue(env, async_req->extra_ref);
    if (async_req->active_request_token != nullptr) {
      EdgeUnregisterActiveRequestToken(env, async_req->active_request_token);
      async_req->active_request_token = nullptr;
    }
    if (req != nullptr && oncomplete != nullptr) {
      napi_value null_value = nullptr;
      napi_get_null(env, &null_value);
      napi_value argv[3] = {null_value != nullptr ? null_value : Undefined(env), Undefined(env), extra};
      size_t argc = 1;
      if (result < 0) {
        argv[0] = err != nullptr ? err : Undefined(env);
      } else if (value != nullptr && !IsUndefined(env, value)) {
        argv[1] = value;
        argc = (extra != nullptr && !IsUndefined(env, extra)) ? 3 : 2;
      }
      InvokeFsReqCallback(env, req, oncomplete, argc, argv);
    }
  }

  DestroyAsyncFsReq(async_req);
}

void AfterAsyncFsReq(uv_fs_t* req) {
  auto* async_req = static_cast<AsyncFsReq*>(req != nullptr ? req->data : nullptr);
  if (async_req == nullptr) return;
  if (async_req->env == nullptr) {
    DestroyAsyncFsReq(async_req);
    return;
  }
  edge::HandleScope scope(async_req->env);
  if (!scope.is_open()) {
    DestroyAsyncFsReq(async_req);
    return;
  }
  if (!async_req->delayed_open_completion &&
      ShouldDelayOpenCompletion(async_req, static_cast<int>(req->result))) {
    async_req->delayed_open_completion = true;
    async_req->pending_fifo_writer_open = true;
    FsBindingState* st = GetState(async_req->env);
    if (st != nullptr) {
      st->pending_fifo_writer_opens.emplace(async_req->path_storage, async_req);
    }
    if (ScheduleDeferredOpenCompletion(async_req)) {
      return;
    }
    if (st != nullptr) {
      auto range = st->pending_fifo_writer_opens.equal_range(async_req->path_storage);
      for (auto it = range.first; it != range.second; ++it) {
        if (it->second == async_req) {
          st->pending_fifo_writer_opens.erase(it);
          break;
        }
      }
    }
    async_req->pending_fifo_writer_open = false;
    async_req->delayed_open_completion = false;
  }
  FinishAsyncFsReq(async_req, static_cast<int>(req->result));
}

AsyncFsReq* CreateAsyncFsReq(napi_env env,
                             ReqKind req_kind,
                             napi_value req,
                             napi_value oncomplete,
                             napi_value* promise_out) {
  if (promise_out != nullptr) *promise_out = nullptr;
  auto* async_req = new AsyncFsReq();
  async_req->env = env;
  async_req->req_kind = req_kind;
  async_req->req.data = async_req;

  if (req_kind == ReqKind::kPromise) {
    napi_value promise = nullptr;
    if (napi_create_promise(env, &async_req->deferred, &promise) != napi_ok || promise == nullptr) {
      delete async_req;
      return nullptr;
    }
    if (promise_out != nullptr) *promise_out = promise;
    return async_req;
  }

  if (req_kind == ReqKind::kCallback && req != nullptr && oncomplete != nullptr &&
      napi_create_reference(env, req, 1, &async_req->req_ref) == napi_ok &&
      napi_create_reference(env, oncomplete, 1, &async_req->oncomplete_ref) == napi_ok) {
    async_req->active_request_token =
        EdgeRegisterActiveRequest(env, req, "FSReqCallback", async_req, [](void* data) {
          auto* fs_req = static_cast<AsyncFsReq*>(data);
          if (fs_req == nullptr) return;
          (void)uv_cancel(reinterpret_cast<uv_req_t*>(&fs_req->req));
        }, [](napi_env env_in, void* data) -> napi_value {
          auto* fs_req = static_cast<AsyncFsReq*>(data);
          return fs_req != nullptr ? GetRefValue(env_in, fs_req->req_ref) : nullptr;
        });
    return async_req;
  }

  delete async_req;
  return nullptr;
}

void SetSyncCtxUvError(napi_env env, napi_value ctx, int errorno, const char* syscall) {
  if (ctx == nullptr || IsUndefined(env, ctx)) return;

  napi_value errno_value = nullptr;
  if (napi_create_int32(env, errorno, &errno_value) == napi_ok && errno_value != nullptr) {
    napi_set_named_property(env, ctx, "errno", errno_value);
  }

  if (syscall != nullptr) {
    napi_value syscall_value = nullptr;
    if (napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_value) == napi_ok &&
        syscall_value != nullptr) {
      napi_set_named_property(env, ctx, "syscall", syscall_value);
    }
  }
}

bool SyncWriteWithUv(napi_env env,
                     int32_t fd,
                     uv_buf_t* bufs,
                     size_t nbufs,
                     int64_t position,
                     napi_value ctx,
                     napi_value* out) {
  if (out != nullptr) *out = nullptr;

  uv_fs_t req;
  const int rc = uv_fs_write(nullptr, &req, fd, bufs, nbufs, position, nullptr);
  const int result = rc < 0 ? rc : static_cast<int>(req.result);
  uv_fs_req_cleanup(&req);

  if (result < 0) {
    if (ctx != nullptr && !IsUndefined(env, ctx)) {
      SetSyncCtxUvError(env, ctx, result, "write");
      return false;
    }
    napi_throw(env, CreateUvExceptionValue(env, result, "write"));
    return false;
  }

  if (out != nullptr) napi_create_int64(env, result, out);
  return true;
}

napi_value CreateUvExceptionValue(napi_env env, int errorno, const char* syscall) {
  const char* code = uv_err_name(errorno);
  const char* message = uv_strerror(errorno);
  std::string full_message;
  if (code != nullptr && *code != '\0') {
    full_message.append(code);
    full_message.append(": ");
  }
  full_message.append(message != nullptr ? message : "Unknown system error");
  if (syscall != nullptr && *syscall != '\0') {
    full_message.append(", ");
    full_message.append(syscall);
  }

  napi_value message_value = nullptr;
  napi_value error = nullptr;
  napi_create_string_utf8(env, full_message.c_str(), NAPI_AUTO_LENGTH, &message_value);
  napi_value global = nullptr;
  napi_value error_ctor = nullptr;
  napi_get_global(env, &global);
  if (global != nullptr) {
    napi_get_named_property(env, global, "Error", &error_ctor);
  }
  if (error_ctor != nullptr) {
    napi_new_instance(env, error_ctor, 1, &message_value, &error);
  }
  if (error == nullptr) {
    napi_create_error(env, nullptr, message_value, &error);
  }
  if (error == nullptr) return Undefined(env);

  napi_value errno_value = nullptr;
  napi_create_int32(env, errorno, &errno_value);
  napi_set_named_property(env, error, "errno", errno_value);

  napi_value code_value = nullptr;
  napi_create_string_utf8(env, code != nullptr ? code : "UV_UNKNOWN", NAPI_AUTO_LENGTH, &code_value);
  napi_set_named_property(env, error, "code", code_value);

  if (syscall != nullptr && *syscall != '\0') {
    napi_value syscall_value = nullptr;
    napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_value);
    napi_set_named_property(env, error, "syscall", syscall_value);
  }

  return error;
}

napi_value CreateUvExceptionValue(napi_env env, int errorno, const char* syscall, const char* path) {
  napi_value error = CreateUvExceptionValue(env, errorno, syscall);
  if (error == nullptr || IsUndefined(env, error)) return error;

  std::string message;
  napi_value message_value = nullptr;
  if (path != nullptr && *path != '\0' &&
      napi_get_named_property(env, error, "message", &message_value) == napi_ok &&
      ValueToUtf8(env, message_value, &message)) {
    message.append(" '");
    message.append(path);
    message.push_back('\'');
    napi_value updated_message = nullptr;
    if (napi_create_string_utf8(env, message.c_str(), message.size(), &updated_message) == napi_ok &&
        updated_message != nullptr) {
      napi_set_named_property(env, error, "message", updated_message);
    }
  }

  if (path != nullptr && *path != '\0') {
    napi_value path_value = nullptr;
    if (napi_create_string_utf8(env, path, NAPI_AUTO_LENGTH, &path_value) == napi_ok && path_value != nullptr) {
      napi_set_named_property(env, error, "path", path_value);
    }
  }

  return error;
}

void FinishFileHandleClose(FileHandleCloseReq* close_req, int result) {
  if (close_req == nullptr) return;
  FileHandleWrap* wrap = close_req->wrap;
  napi_env env = close_req->env;
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY fs FileHandleClose finish fd=%d result=%d deferred=%p\n",
                 wrap != nullptr ? wrap->fd : -1,
                 result,
                 wrap != nullptr ? wrap->closing_deferred : nullptr);
  }

  auto delete_close_req = [env](FileHandleCloseReq* req) {
    if (req == nullptr) return;
    if (env != nullptr && req->active_request_token != nullptr) {
      EdgeUnregisterActiveRequestToken(env, req->active_request_token);
      req->active_request_token = nullptr;
    }
    if (env != nullptr) ResetRef(env, &req->req_ref);
    uv_fs_req_cleanup(&req->req);
    delete req;
  };

  auto after_close = [](FileHandleWrap* handle) {
    if (handle == nullptr) return;
    const bool emit_eof = handle->reading;
    handle->closing = false;
    handle->closed = true;
    handle->reading = false;
    handle->fd = -1;
    EdgeStreamBaseSetReading(&handle->base, false);
    if (emit_eof) {
      (void)EdgeStreamBaseEmitEOF(&handle->base);
    }
  };

  if (env != nullptr && EdgeWorkerEnvStopRequested(env)) {
    if (wrap != nullptr) {
      after_close(wrap);
      wrap->closing_deferred = nullptr;
      ResetRef(env, &wrap->closing_promise_ref);
      ReleaseFileHandleRef(wrap);
    }
    delete_close_req(close_req);
    return;
  }

  if (wrap != nullptr) {
    if (result >= 0) {
      after_close(wrap);
    } else {
      wrap->closing = false;
    }
  }

  if (close_req->is_shutdown) {
    napi_value req_obj = env != nullptr ? GetRefValue(env, close_req->req_ref) : nullptr;
    if (wrap != nullptr && req_obj != nullptr) {
      EdgeStreamBaseEmitAfterShutdown(&wrap->base, req_obj, result);
    }
    if (wrap != nullptr) {
      ReleaseFileHandleRef(wrap);
    }
    delete_close_req(close_req);
    return;
  }

  if (env != nullptr && wrap != nullptr && wrap->closing_deferred != nullptr) {
    if (result < 0) {
      napi_value err = CreateUvExceptionValue(env, result, "close");
      (void)napi_reject_deferred(env, wrap->closing_deferred, err);
    } else {
      (void)napi_resolve_deferred(env, wrap->closing_deferred, Undefined(env));
    }
    wrap->closing_deferred = nullptr;
    (void)EdgeRunCallbackScopeCheckpoint(env);
  }

  if (env != nullptr && wrap != nullptr) {
    ResetRef(env, &wrap->closing_promise_ref);
    ReleaseFileHandleRef(wrap);
  }

  delete_close_req(close_req);
}

void AfterFileHandleClose(uv_fs_t* req) {
  auto* close_req = static_cast<FileHandleCloseReq*>(req != nullptr ? req->data : nullptr);
  if (close_req == nullptr) return;
  napi_env env = close_req->wrap != nullptr ? close_req->wrap->env : nullptr;
  if (env != nullptr) {
    edge::HandleScope scope(env);
    if (!scope.is_open()) {
      if (close_req->active_request_token != nullptr) {
        EdgeUnregisterActiveRequestToken(env, close_req->active_request_token);
        close_req->active_request_token = nullptr;
      }
      ResetRef(env, &close_req->req_ref);
      uv_fs_req_cleanup(&close_req->req);
      delete close_req;
      return;
    }
    if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
      auto* wrap = close_req->wrap;
      std::fprintf(stderr,
                   "EDGE_TRACE_TTY fs FileHandleClose after fd=%d result=%zd\n",
                   wrap != nullptr ? wrap->fd : -1,
                   req != nullptr ? req->result : -1);
    }
    FinishFileHandleClose(close_req, static_cast<int>(req->result));
    return;
  }
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    auto* wrap = close_req->wrap;
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY fs FileHandleClose after fd=%d result=%zd\n",
                 wrap != nullptr ? wrap->fd : -1,
                 req != nullptr ? req->result : -1);
  }
  FinishFileHandleClose(close_req, static_cast<int>(req->result));
}

FileHandleWrap* FileHandleFromBase(EdgeStreamBase* base) {
  if (base == nullptr) return nullptr;
  return reinterpret_cast<FileHandleWrap*>(reinterpret_cast<char*>(base) - offsetof(FileHandleWrap, base));
}

void DestroyFileHandleBase(EdgeStreamBase* base) {
  delete FileHandleFromBase(base);
}

int FileHandleReadStopInternal(FileHandleWrap* wrap);

void FinishFileHandleRead(FileHandleReadReq* read_req, uv_fs_t* req) {
  if (read_req == nullptr) return;

  FileHandleWrap* wrap = read_req->wrap;
  if (wrap != nullptr && wrap->env != nullptr && read_req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(wrap->env, read_req->active_request_token);
    read_req->active_request_token = nullptr;
  }
  ssize_t result = req->result;
  uv_fs_req_cleanup(req);

  if (wrap != nullptr && wrap->current_read == read_req) {
    wrap->current_read = nullptr;
  }

  uv_buf_t buf = uv_buf_init(read_req->storage, read_req->buf.len);
  read_req->storage = nullptr;

  if (wrap != nullptr && result >= 0) {
    if (wrap->read_length >= 0 && static_cast<int64_t>(result) > wrap->read_length) {
      result = wrap->read_length;
    }
    if (wrap->read_length >= 0) {
      wrap->read_length -= result;
    }
    if (wrap->read_offset >= 0) {
      wrap->read_offset += result;
    }
  }

  if (result == 0) result = UV_EOF;

  if (wrap != nullptr) {
    if (result > 0) {
      buf.len = static_cast<unsigned int>(result);
      EdgeStreamBaseOnUvRead(&wrap->base, result, &buf);
    } else {
      if (buf.base != nullptr) free(buf.base);
      EdgeStreamBaseOnUvRead(&wrap->base, result, nullptr);
    }
    ReleaseFileHandleRef(wrap);
    if (result > 0 && wrap->reading && !wrap->closing && !wrap->closed) {
      (void)EdgeStreamBaseReadStart(&wrap->base);
    }
  } else if (buf.base != nullptr) {
    free(buf.base);
  }

  delete read_req;
}

void AfterFileHandleRead(uv_fs_t* req) {
  auto* read_req = static_cast<FileHandleReadReq*>(req != nullptr ? req->data : nullptr);
  if (read_req == nullptr) return;

  napi_env env = read_req->wrap != nullptr ? read_req->wrap->env : nullptr;
  if (env == nullptr) {
    FinishFileHandleRead(read_req, req);
    return;
  }

  edge::HandleScope scope(env);
  if (!scope.is_open()) {
    if (read_req->storage != nullptr) free(read_req->storage);
    uv_fs_req_cleanup(req);
    delete read_req;
    return;
  }

  FinishFileHandleRead(read_req, req);
}

int FileHandleReadStartInternal(FileHandleWrap* wrap) {
  if (wrap == nullptr) return UV_EBADF;
  if (wrap->fd < 0 || wrap->closed || wrap->closing) return UV_EOF;

  wrap->reading = true;
  EdgeStreamBaseSetReading(&wrap->base, true);

  if (wrap->current_read != nullptr) return 0;

  if (wrap->read_length == 0) {
    (void)EdgeStreamBaseEmitEOF(&wrap->base);
    return 0;
  }

  size_t suggested = 65536;
  if (wrap->read_length >= 0 && wrap->read_length < static_cast<int64_t>(suggested)) {
    suggested = static_cast<size_t>(wrap->read_length);
  }
  if (suggested == 0) suggested = 1;

  auto* read_req = new FileHandleReadReq();
  read_req->wrap = wrap;
  read_req->storage = static_cast<char*>(malloc(suggested));
  if (read_req->storage == nullptr) {
    delete read_req;
    return UV_ENOMEM;
  }
  read_req->buf = uv_buf_init(read_req->storage, static_cast<unsigned int>(suggested));
  read_req->req.data = read_req;

  uv_loop_t* loop = EdgeGetEnvLoop(wrap->env);
  if (loop == nullptr) {
    free(read_req->storage);
    delete read_req;
    return UV_EINVAL;
  }

  wrap->current_read = read_req;
  napi_value owner = GetFileHandleOwner(wrap->env, wrap);
  if (owner != nullptr) {
    read_req->active_request_token =
        EdgeRegisterActiveRequest(wrap->env,
                                  owner,
                                  "FSReqCallback",
                                  read_req,
                                  CancelFileHandleReadReq,
                                  GetFileHandleReadReqOwner);
  }
  HoldFileHandleRef(wrap);
  const int rc = uv_fs_read(loop,
                            &read_req->req,
                            wrap->fd,
                            &read_req->buf,
                            1,
                            wrap->read_offset,
                            AfterFileHandleRead);
  if (rc < 0) {
    wrap->current_read = nullptr;
    if (read_req->active_request_token != nullptr) {
      EdgeUnregisterActiveRequestToken(wrap->env, read_req->active_request_token);
      read_req->active_request_token = nullptr;
    }
    ReleaseFileHandleRef(wrap);
    free(read_req->storage);
    delete read_req;
    return rc;
  }

  return 0;
}

int FileHandleReadStartOp(EdgeStreamBase* base) {
  return FileHandleReadStartInternal(FileHandleFromBase(base));
}

int FileHandleReadStopInternal(FileHandleWrap* wrap) {
  if (wrap == nullptr) return UV_EBADF;
  wrap->reading = false;
  EdgeStreamBaseSetReading(&wrap->base, false);
  return 0;
}

int FileHandleReadStopOp(EdgeStreamBase* base) {
  return FileHandleReadStopInternal(FileHandleFromBase(base));
}

int FileHandleShutdownOp(EdgeStreamBase* base, napi_value req_obj) {
  FileHandleWrap* wrap = FileHandleFromBase(base);
  if (wrap == nullptr || req_obj == nullptr) return UV_EINVAL;
  if (wrap->closing || wrap->closed || wrap->fd < 0) {
    EdgeStreamBaseInvokeReqOnComplete(wrap->env, req_obj, 0, nullptr, 0);
    return 1;
  }

  auto* close_req = new FileHandleCloseReq();
  close_req->env = wrap->env;
  close_req->wrap = wrap;
  close_req->is_shutdown = true;
  close_req->req.data = close_req;
  if (napi_create_reference(wrap->env, req_obj, 1, &close_req->req_ref) != napi_ok) {
    delete close_req;
    return UV_EINVAL;
  }

  wrap->closing = true;
  EdgeStreamReqActivate(wrap->env, req_obj, kEdgeProviderShutdownWrap, wrap->base.async_id);
  HoldFileHandleRef(wrap);
  close_req->active_request_token =
      EdgeRegisterActiveRequest(wrap->env,
                                req_obj,
                                "FSReqCallback",
                                close_req,
                                CancelFileHandleCloseReq,
                                GetFileHandleCloseReqOwner);

  uv_loop_t* loop = EdgeGetEnvLoop(wrap->env);
  const int rc = loop != nullptr ? uv_fs_close(loop, &close_req->req, wrap->fd, AfterFileHandleClose)
                                 : UV_EINVAL;
  if (rc < 0) {
    FinishFileHandleClose(close_req, rc);
  }
  return rc;
}

const EdgeStreamBaseOps kFileHandleStreamOps = {
    nullptr,
    nullptr,
    nullptr,
    DestroyFileHandleBase,
    nullptr,
    FileHandleReadStartOp,
    FileHandleReadStopOp,
    FileHandleShutdownOp,
    nullptr,
};

FileHandleWrap* UnwrapFileHandle(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<FileHandleWrap*>(data);
}

void FileHandleFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<FileHandleWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->fd >= 0 && !wrap->closed && !wrap->closing) {
    const int fd = wrap->fd;
    const int rc = ::close(fd);
    if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
      const int close_status = rc == 0 ? 0 : -(errno != 0 ? errno : UV_EIO);
      environment->QueueFileHandleGcWarning(fd, close_status);
    }
    wrap->fd = -1;
    wrap->closed = (rc == 0);
  }
  ResetRef(env, &wrap->closing_promise_ref);
  EdgeStreamBaseFinalize(&wrap->base);
}

napi_value FileHandleCtor(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new FileHandleWrap();
  wrap->env = env;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &wrap->fd);
  int64_t offset = -1;
  int64_t length = -1;
  if (argc >= 2 && argv[1] != nullptr) (void)napi_get_value_int64(env, argv[1], &offset);
  if (argc >= 3 && argv[2] != nullptr) (void)napi_get_value_int64(env, argv[2], &length);
  wrap->read_offset = offset;
  wrap->read_length = length;
  if (wrap->fd >= 0) {
    EdgeWorkerEnvRemoveUnmanagedFd(env, wrap->fd);
  }
  EdgeStreamBaseInit(&wrap->base, env, &kFileHandleStreamOps, kEdgeProviderNone);
  napi_value offset_value = nullptr;
  napi_value length_value = nullptr;
  napi_create_int64(env, offset, &offset_value);
  napi_create_int64(env, length, &length_value);
  if (napi_wrap(env, this_arg, wrap, FileHandleFinalize, nullptr, &wrap->base.wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->base.wrapper_ref);
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, false, false);
  if (offset_value != nullptr) napi_set_named_property(env, this_arg, "offset", offset_value);
  if (length_value != nullptr) napi_set_named_property(env, this_arg, "length", length_value);
  return this_arg;
}

napi_value FileHandleGetFd(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  napi_value out = nullptr;
  napi_create_int32(env, wrap == nullptr ? -1 : wrap->fd, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FileHandleGetBytesRead(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseGetBytesRead(&wrap->base) : Undefined(env);
}

napi_value FileHandleGetBytesWritten(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseGetBytesWritten(&wrap->base) : Undefined(env);
}

napi_value FileHandleGetExternalStream(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseGetExternal(&wrap->base) : Undefined(env);
}

napi_value FileHandleGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseGetAsyncId(&wrap->base) : Undefined(env);
}

napi_value FileHandleGetOnread(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseGetOnRead(&wrap->base) : Undefined(env);
}

napi_value FileHandleSetOnread(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  return EdgeStreamBaseSetOnRead(&wrap->base, argc >= 1 ? argv[0] : Undefined(env));
}

napi_value FileHandleClose(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);

  if (wrap->fd < 0 || wrap->closed) return MakeResolvedPromise(env, Undefined(env));
  if (wrap->current_read != nullptr) {
    return MakeRejectedPromise(env, CreateUvExceptionValue(env, UV_EBUSY, "close"));
  }

  napi_value closing_promise = GetRefValue(env, wrap->closing_promise_ref);
  if (closing_promise != nullptr && wrap->closing) return closing_promise;

  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) return Undefined(env);

  napi_create_reference(env, promise, 1, &wrap->closing_promise_ref);
  wrap->closing_deferred = deferred;
  wrap->closing = true;
  wrap->reading = false;
  EdgeStreamBaseSetReading(&wrap->base, false);

  auto* close_req = new FileHandleCloseReq();
  close_req->env = env;
  close_req->wrap = wrap;
  close_req->req.data = close_req;
  close_req->active_request_token =
      EdgeRegisterActiveRequest(env,
                                this_arg,
                                "FSReqCallback",
                                close_req,
                                CancelFileHandleCloseReq,
                                GetFileHandleCloseReqOwner);
  HoldFileHandleRef(wrap);

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  const int rc = loop != nullptr ? uv_fs_close(loop, &close_req->req, wrap->fd, AfterFileHandleClose)
                                 : UV_EINVAL;
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY fs FileHandleClose start fd=%d rc=%d loop=%p\n",
                 wrap->fd,
                 rc,
                 static_cast<void*>(loop));
  }
  if (rc < 0) {
    FinishFileHandleClose(close_req, rc);
  }

  return promise;
}

napi_value FileHandleReleaseFD(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  int32_t old_fd = wrap == nullptr ? -1 : wrap->fd;
  if (wrap != nullptr) {
    const bool emit_eof = wrap->reading;
    wrap->fd = -1;
    wrap->closing = false;
    wrap->closed = true;
    wrap->reading = false;
    EdgeStreamBaseSetReading(&wrap->base, false);
    if (emit_eof) {
      (void)EdgeStreamBaseEmitEOF(&wrap->base);
    }
  }
  napi_value out = nullptr;
  napi_create_int32(env, old_fd, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FileHandleReadStart(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return MakeInt32(env, FileHandleReadStartInternal(wrap));
}

napi_value FileHandleReadStop(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return MakeInt32(env, FileHandleReadStopInternal(wrap));
}

napi_value FileHandleShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return MakeInt32(env, wrap != nullptr ? FileHandleShutdownOp(&wrap->base, argc >= 1 ? argv[0] : nullptr)
                                        : UV_EINVAL);
}

napi_value FileHandleUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseUseUserBuffer(&wrap->base, argc >= 1 ? argv[0] : Undefined(env))
                         : Undefined(env);
}

napi_value CallBindingMethodByName(napi_env env, napi_value binding, const char* name, size_t argc, napi_value* argv) {
  napi_value fn = nullptr;
  napi_valuetype t = napi_undefined;
  if (napi_get_named_property(env, binding, name, &fn) != napi_ok ||
      fn == nullptr ||
      napi_typeof(env, fn, &t) != napi_ok ||
      t != napi_function) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  if (napi_call_function(env, binding, fn, argc, argv, &out) != napi_ok || out == nullptr) return Undefined(env);
  return out;
}

napi_value FileHandleWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value fd_value = nullptr;
  napi_create_int32(env, wrap->fd, &fd_value);
  napi_value call_argv[4] = {fd_value, argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  return CallBindingMethodByName(env, binding, "writeBuffers", 4, call_argv);
}

napi_value FileHandleWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value fd_value = nullptr;
  napi_create_int32(env, wrap->fd, &fd_value);
  napi_value call_argv[6] = {fd_value,
                             argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env),
                             argc >= 4 ? argv[3] : Undefined(env),
                             argc >= 5 ? argv[4] : Undefined(env)};
  return CallBindingMethodByName(env, binding, "writeBuffer", 6, call_argv);
}

napi_value FileHandleWriteStringWithEncoding(napi_env env,
                                             napi_callback_info info,
                                             const char* encoding) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_value binding = GetBinding(env);
  if (binding == nullptr) return Undefined(env);
  napi_value fd_value = nullptr;
  napi_create_int32(env, wrap->fd, &fd_value);
  napi_value enc_value = nullptr;
  napi_create_string_utf8(env, encoding, NAPI_AUTO_LENGTH, &enc_value);
  napi_value call_argv[5] = {fd_value, argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env),
                             enc_value, argc >= 3 ? argv[2] : Undefined(env)};
  return CallBindingMethodByName(env, binding, "writeString", 5, call_argv);
}

napi_value FileHandleWriteAsciiString(napi_env env, napi_callback_info info) {
  return FileHandleWriteStringWithEncoding(env, info, "ascii");
}

napi_value FileHandleWriteUtf8String(napi_env env, napi_callback_info info) {
  return FileHandleWriteStringWithEncoding(env, info, "utf8");
}

napi_value FileHandleWriteUcs2String(napi_env env, napi_callback_info info) {
  return FileHandleWriteStringWithEncoding(env, info, "ucs2");
}

napi_value FileHandleWriteLatin1String(napi_env env, napi_callback_info info) {
  return FileHandleWriteStringWithEncoding(env, info, "latin1");
}

napi_value FileHandleIsStreamBase(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  FileHandleWrap* wrap = UnwrapFileHandle(env, this_arg);
  return wrap != nullptr ? EdgeStreamBaseMakeBool(env, true) : Undefined(env);
}

struct StatWatcherWrap {
  EdgeHandleWrap handle_wrap{};
  uv_fs_poll_t handle{};
  bool referenced = true;
  bool use_bigint = false;
  int64_t async_id = 0;
};

bool StatWatcherHasRef(void* data) {
  auto* wrap = static_cast<StatWatcherWrap*>(data);
  return wrap != nullptr &&
         EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle));
}

napi_value StatWatcherGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<StatWatcherWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

StatWatcherWrap* UnwrapStatWatcher(napi_env env, napi_value this_arg) {
  if (env == nullptr || this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<StatWatcherWrap*>(data);
}

void DeleteStatWatcherWrap(void* data) {
  delete static_cast<StatWatcherWrap*>(data);
}

void OnStatWatcherClosed(uv_handle_t* handle) {
  auto* wrap = static_cast<StatWatcherWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr) return;
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteStatWatcherWrap);
}

void CloseStatWatcher(StatWatcherWrap* wrap) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  wrap->handle_wrap.state = kEdgeHandleClosing;
  uv_close(reinterpret_cast<uv_handle_t*>(&wrap->handle), OnStatWatcherClosed);
}

void CloseStatWatcherForCleanup(void* data) {
  CloseStatWatcher(static_cast<StatWatcherWrap*>(data));
}

void OnStatWatcherChange(uv_fs_poll_t* handle, int status, const uv_stat_t* prev, const uv_stat_t* curr) {
  auto* wrap = static_cast<StatWatcherWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr || wrap->handle_wrap.env == nullptr) return;
  edge::HandleScope scope(wrap->handle_wrap.env);
  if (!scope.is_open()) return;

  napi_value self = EdgeHandleWrapGetRefValue(wrap->handle_wrap.env, wrap->handle_wrap.wrapper_ref);
  if (self == nullptr) return;
  napi_value onchange = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(wrap->handle_wrap.env, self, "onchange", &onchange) != napi_ok ||
      onchange == nullptr ||
      napi_typeof(wrap->handle_wrap.env, onchange, &type) != napi_ok ||
      type != napi_function) {
    return;
  }

  uv_stat_t curr_copy{};
  uv_stat_t prev_copy{};
  if (curr != nullptr) curr_copy = *curr;
  if (prev != nullptr) prev_copy = *prev;

  napi_value argv[2] = {nullptr, nullptr};
  napi_create_int32(wrap->handle_wrap.env, status, &argv[0]);
  argv[1] = CreateStatWatcherArray(wrap->handle_wrap.env, wrap->use_bigint, &curr_copy, &prev_copy);

  napi_value ignored = nullptr;
  EdgeMakeCallback(wrap->handle_wrap.env, self, onchange, 2, argv, &ignored);
}

void StatWatcherFinalize(napi_env env, void* data, void* /*hint*/) {
  EdgeHandleWrapFinalizeNative(env, &static_cast<StatWatcherWrap*>(data)->handle_wrap, data, DeleteStatWatcherWrap);
}

napi_value StatWatcherCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;
  auto* wrap = new StatWatcherWrap();
  EdgeHandleWrapInit(&wrap->handle_wrap, env);
  wrap->async_id = g_next_stat_watcher_async_id++;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_bool(env, argv[0], &wrap->use_bigint);
  }
  if (napi_wrap(env, this_arg, wrap, StatWatcherFinalize, nullptr, &wrap->handle_wrap.wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  return this_arg;
}

napi_value StatWatcherStart(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, UV_EINVAL);
  if (wrap->handle_wrap.state == kEdgeHandleInitialized) return MakeInt32(env, 0);
  if (wrap->handle_wrap.state == kEdgeHandleClosing || wrap->handle_wrap.state == kEdgeHandleClosed) {
    return MakeInt32(env, UV_EINVAL);
  }

  std::string path;
  if (!ValueToUtf8(env, argv[0], &path)) return MakeInt32(env, UV_EINVAL);
  uint32_t interval = 0;
  if (napi_get_value_uint32(env, argv[1], &interval) != napi_ok) return MakeInt32(env, UV_EINVAL);

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  int rc = loop != nullptr ? uv_fs_poll_init(loop, &wrap->handle) : UV_EINVAL;
  if (rc != 0) return MakeInt32(env, rc);

  wrap->handle.data = wrap;
  EdgeHandleWrapAttach(&wrap->handle_wrap,
                      wrap,
                      reinterpret_cast<uv_handle_t*>(&wrap->handle),
                      CloseStatWatcherForCleanup);
  rc = uv_fs_poll_start(&wrap->handle, OnStatWatcherChange, path.c_str(), interval);
  if (rc != 0) {
    wrap->handle_wrap.state = kEdgeHandleClosing;
    uv_close(reinterpret_cast<uv_handle_t*>(&wrap->handle), OnStatWatcherClosed);
    return MakeInt32(env, rc);
  }

  wrap->handle_wrap.state = kEdgeHandleInitialized;
  // Keep-alive comes from EdgeRegisterActiveHandle's strong keepalive_ref.
  if (wrap->handle_wrap.active_handle_token == nullptr) {
    wrap->handle_wrap.active_handle_token =
        EdgeRegisterActiveHandle(env,
                                 this_arg,
                                 "StatWatcher",
                                 StatWatcherHasRef,
                                 StatWatcherGetActiveOwner,
                                 wrap,
                                 CloseStatWatcherForCleanup);
  }
  wrap->referenced = true;
  return MakeInt32(env, 0);
}

napi_value StatWatcherClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized && argc >= 1 && argv[0] != nullptr) {
    EdgeHandleWrapSetOnCloseCallback(env, this_arg, argv[0]);
  }
  if (wrap != nullptr) CloseStatWatcher(wrap);
  return Undefined(env);
}

napi_value StatWatcherRef(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized && !wrap->referenced) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
    wrap->referenced = true;
  }
  return Undefined(env);
}

napi_value StatWatcherUnref(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized && wrap->referenced) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
    wrap->referenced = false;
  }
  return Undefined(env);
}

napi_value StatWatcherHasRef(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  napi_value out = nullptr;
  napi_get_boolean(
      env,
      wrap != nullptr &&
          EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle)),
      &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StatWatcherGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  StatWatcherWrap* wrap = UnwrapStatWatcher(env, this_arg);
  napi_value out = nullptr;
  napi_create_int64(env, wrap == nullptr ? -1 : wrap->async_id, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FSReqCallbackCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return Undefined(env);

  auto* wrap = new FsReqCallbackWrap();
  wrap->env = env;
  wrap->async_id = EdgeAsyncWrapNextId(env);
  if (napi_wrap(env, this_arg, wrap, FSReqCallbackFinalize, nullptr, nullptr) != napi_ok) {
    delete wrap;
    return this_arg;
  }
  EdgeAsyncWrapEmitInitString(
      env, wrap->async_id, "FSREQCALLBACK", EdgeAsyncWrapExecutionAsyncId(env), this_arg);
  return this_arg != nullptr ? this_arg : Undefined(env);
}

napi_value FsAccess(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  int32_t validated_mode = 0;
  if (argc >= 2 && argv[1] != nullptr && !ValidateAccessModeArg(env, argv[1], &validated_mode)) return nullptr;

  napi_value mode_value = nullptr;
  if (argc >= 2 && argv[1] != nullptr) napi_create_int32(env, validated_mode, &mode_value);

  napi_value call_argv[2] = {
      argc >= 1 ? argv[0] : Undefined(env),
      mode_value != nullptr ? mode_value : (argc >= 2 ? argv[1] : Undefined(env)),
  };
  napi_value ignored = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "accessSync", 2, call_argv, &ignored, &err)) {
    // Prefer raw async access when available, but do not erase sync failure
    // semantics when only accessSync exists.
    FsBindingState* st = GetState(env);
    const bool has_async_access =
        st != nullptr && st->raw_methods.find("access") != st->raw_methods.end();
    if (has_async_access) {
      napi_value async_err = nullptr;
      if (CallRaw(env, "access", 2, call_argv, &ignored, &async_err)) {
        if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
        CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
        return Undefined(env);
      }
      if (async_err != nullptr) {
        err = async_err;
      }
    }
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
  return Undefined(env);
}

napi_value FsStatCommon(napi_env env,
                        napi_callback_info info,
                        const char* raw_name,
                        bool allow_throw_if_no_entry,
                        size_t stats_len) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  bool use_bigint = false;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_bool(env, argv[1], &use_bigint);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  bool throw_if_no_entry = true;
  if (allow_throw_if_no_entry && argc >= 4 && argv[3] != nullptr) {
    napi_get_value_bool(env, argv[3], &throw_if_no_entry);
  }
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, raw_name, 1, call_argv, &raw_out, &err)) {
    if (std::strcmp(raw_name, "stat") == 0 &&
        (ErrorCodeEquals(env, err, "ENOENT") || ErrorCodeEquals(env, err, "ENOTDIR"))) {
      std::string path;
      if (ValueToPathString(env, argc >= 1 ? argv[0] : nullptr, &path)) {
        const std::filesystem::path resolved = ResolveSymlinkComponentsForModuleResolution(path);
        if (resolved != std::filesystem::path(path).lexically_normal()) {
          napi_value resolved_arg = nullptr;
          const std::string resolved_path = resolved.string();
          napi_create_string_utf8(env, resolved_path.c_str(), resolved_path.size(), &resolved_arg);
          napi_value retry_out = nullptr;
          napi_value retry_err = nullptr;
          napi_value retry_argv[1] = {resolved_arg != nullptr ? resolved_arg : call_argv[0]};
          if (CallRaw(env, raw_name, 1, retry_argv, &retry_out, &retry_err)) {
            raw_out = retry_out;
            err = nullptr;
          } else if (retry_err != nullptr) {
            err = retry_err;
          }
        }
      }
    }
  }

  if (raw_out == nullptr && err != nullptr) {
    if (!throw_if_no_entry &&
        (ErrorCodeEquals(env, err, "ENOENT") || ErrorCodeEquals(env, err, "ENOTDIR"))) {
      if (req_kind == ReqKind::kPromise) return MakeDeferredResolvedPromise(env, Undefined(env));
      CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
      return Undefined(env);
    }
    if (req_kind == ReqKind::kPromise) return MakeDeferredRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  napi_value typed = CreateTypedStatsArray(env, stats_len, use_bigint, raw_out);
  if (req_kind == ReqKind::kPromise) return MakeDeferredResolvedPromise(env, typed);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, typed);
  return typed;
}

napi_value FsStat(napi_env env, napi_callback_info info) {
  return FsStatCommon(env, info, "stat", true, kFsStatsLength);
}

napi_value FsLstat(napi_env env, napi_callback_info info) {
  return FsStatCommon(env, info, "lstat", true, kFsStatsLength);
}

napi_value FsFstat(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  return FsStatCommon(env, info, "fstat", false, kFsStatsLength);
}

napi_value FsStatfs(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool use_bigint = false;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_bool(env, argv[1], &use_bigint);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "statfs", 1, call_argv, &raw_out, &err)) {
    // Node exposes statfs even if platform support is partial; return zeroed stats
    // for environments without a native statfs implementation.
    napi_value typed = CreateTypedStatsArray(env, kFsStatFsLength, use_bigint, nullptr);
    if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, typed);
    CompleteReq(env, req_kind, req, oncomplete, nullptr, typed);
    return typed;
  }
  napi_value typed = CreateTypedStatsArray(env, kFsStatFsLength, use_bigint, raw_out);
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, typed);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, typed);
  return typed;
}

napi_value FsReaddir(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  bool with_file_types = false;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &with_file_types);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  napi_value with_file_types_value = nullptr;
  napi_get_boolean(env, with_file_types, &with_file_types_value);
  napi_value call_argv[2] = {
      argc >= 1 ? argv[0] : Undefined(env),
      with_file_types_value != nullptr ? with_file_types_value : Undefined(env),
  };
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "readdir", 2, call_argv, &raw_out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kNone && err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  napi_value out = raw_out;
  if (with_file_types) {
    napi_value names = nullptr;
    napi_value types = nullptr;
    if (raw_out != nullptr &&
        napi_get_element(env, raw_out, 0, &names) == napi_ok &&
        napi_get_element(env, raw_out, 1, &types) == napi_ok) {
      napi_value encoded_names = ConvertNameArrayToEncoding(env, names, argc >= 2 ? argv[1] : nullptr);
      napi_value pair = nullptr;
      if (napi_create_array_with_length(env, 2, &pair) == napi_ok && pair != nullptr) {
        napi_set_element(env, pair, 0, encoded_names != nullptr ? encoded_names : names);
        napi_set_element(env, pair, 1, types != nullptr ? types : Undefined(env));
        out = pair;
      }
    }
  } else {
    out = ConvertNameArrayToEncoding(env, raw_out, argc >= 2 ? argv[1] : nullptr);
  }

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
  return out;
}

napi_value FsMkdtemp(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  const bool as_buffer = argc >= 2 && argv[1] != nullptr && IsBufferEncoding(env, argv[1]);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "mkdtemp", 1, call_argv, &raw_out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kNone && err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  napi_value out = as_buffer ? BufferFromValue(env, raw_out, "utf8") : raw_out;
  if (out == nullptr || IsUndefined(env, out)) out = raw_out;
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
  return out;
}

napi_value FsMkdir(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  std::string path;
  if (!ValueToPathString(env, argc >= 1 ? argv[0] : nullptr, &path)) {
    napi_value err = CreateUvExceptionValue(env, UV_EINVAL, "mkdir");
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    napi_throw(env, err);
    return nullptr;
  }

  path = edge_path::ToNamespacedPath(path);
  int32_t mode = 0;
  bool recursive = false;
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &mode);
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &recursive);

  int err = 0;
  std::string first_path;
  if (recursive) {
    std::vector<std::string> stack;
    stack.push_back(path);
    uv_fs_t uv_req;
    while (!stack.empty()) {
      std::string next_path = std::move(stack.back());
      stack.pop_back();

      err = uv_fs_mkdir(nullptr, &uv_req, next_path.c_str(), mode, nullptr);
      uv_fs_req_cleanup(&uv_req);

      while (true) {
        switch (err) {
          case 0:
            if (first_path.empty()) first_path = next_path;
            break;
          case UV_EACCES:
          case UV_ENOSPC:
          case UV_ENOTDIR:
          case UV_EPERM:
            break;
          case UV_ENOENT: {
            const auto parent = std::filesystem::path(next_path).parent_path();
            const std::string dirname = parent.empty() ? next_path : parent.string();
            if (dirname != next_path) {
              stack.push_back(next_path);
              stack.push_back(dirname);
              err = 0;
            } else if (stack.empty()) {
              err = UV_EEXIST;
              continue;
            }
            break;
          }
          default: {
            const int orig_err = err;
            err = uv_fs_stat(nullptr, &uv_req, next_path.c_str(), nullptr);
            const uv_stat_t statbuf = uv_req.statbuf;
            uv_fs_req_cleanup(&uv_req);
            if (err == 0 && !S_ISDIR(statbuf.st_mode)) {
              err = (orig_err == UV_EEXIST && !stack.empty()) ? UV_ENOTDIR : UV_EEXIST;
            } else if (err == 0) {
              err = 0;
            }
            break;
          }
        }
        break;
      }

      if (err < 0) break;
    }
  } else {
    uv_fs_t uv_req;
    err = uv_fs_mkdir(nullptr, &uv_req, path.c_str(), mode, nullptr);
    uv_fs_req_cleanup(&uv_req);
  }

  if (err < 0) {
    napi_value error = CreateUvExceptionValue(env, err, "mkdir", path.c_str());
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, error);
    CompleteReq(env, req_kind, req, oncomplete, error, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    napi_throw(env, error);
    return nullptr;
  }

  napi_value out = Undefined(env);
  if (!first_path.empty()) {
    napi_create_string_utf8(env, first_path.c_str(), first_path.size(), &out);
  }
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out != nullptr ? out : Undefined(env));
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, out != nullptr ? out : Undefined(env));
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value FsRename(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "rename", req_kind, req, oncomplete, 2, call_argv);
}

napi_value FsCopyFile(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  const bool has_mode = argc >= 3 && argv[2] != nullptr && !IsNullOrUndefined(env, argv[2]);
  int32_t mode = 0;
  if (has_mode && !ValidateCopyFileModeArg(env, argv[2], &mode)) return nullptr;
  napi_value mode_value = nullptr;
  if (has_mode) napi_create_int32(env, mode, &mode_value);
  napi_value call_argv[3] = {
      argc >= 1 ? argv[0] : Undefined(env),
      argc >= 2 ? argv[1] : Undefined(env),
      has_mode && mode_value != nullptr ? mode_value : Undefined(env),
  };
  return CompleteVoidRawFsMethod(env, "copyFile", req_kind, req, oncomplete, has_mode ? 3 : 2, call_argv);
}

napi_value FsReadlink(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  std::string encoding = "utf8";
  if (argc >= 2 && argv[1] != nullptr) ValueToUtf8(env, argv[1], &encoding);

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "readlink", 1, call_argv, &raw_out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  napi_value out = raw_out;
  if (encoding == "buffer") out = BufferFromValue(env, raw_out, nullptr);
  if (out == nullptr || IsUndefined(env, out)) out = raw_out;

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value FsSymlink(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  std::string target;
  std::string path;
  if (!ValueToPathString(env, argc >= 1 ? argv[0] : nullptr, &target) ||
      !ValueToPathString(env, argc >= 2 ? argv[1] : nullptr, &path)) {
    napi_value err = CreateUvExceptionValue(env, UV_EINVAL, "symlink");
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    napi_throw(env, err);
    return nullptr;
  }

  path = edge_path::ToNamespacedPath(path);
  int32_t flags = 0;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &flags);

  int err = 0;
#if defined(_WIN32)
  uv_fs_t uv_req;
  err = uv_fs_symlink(nullptr, &uv_req, target.c_str(), path.c_str(), flags, nullptr);
  uv_fs_req_cleanup(&uv_req);
#else
  (void)flags;
  if (::symlink(target.c_str(), path.c_str()) != 0) {
    err = uv_translate_sys_error(errno);
  }
#endif

  if (err < 0) {
    napi_value error = CreateUvExceptionValue(env, err, "symlink", path.c_str());
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, error);
    CompleteReq(env, req_kind, req, oncomplete, error, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    napi_throw(env, error);
    return nullptr;
  }

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
    return Undefined(env);
  }
  return Undefined(env);
}

napi_value FsUnlink(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 2 ? argv[1] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "unlink", req_kind, req, oncomplete, 1, call_argv);
}

napi_value FsRmdir(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 2 ? argv[1] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "rmdir", req_kind, req, oncomplete, 1, call_argv);
}

napi_value FsFtruncate(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "ftruncate", req_kind, req, oncomplete, 2, call_argv);
}

napi_value FsFsync(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 2 ? argv[1] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "fsync", req_kind, req, oncomplete, 1, call_argv);
}

napi_value FsChmod(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "chmod", req_kind, req, oncomplete, 2, call_argv);
}

napi_value FsFchmod(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "fchmod", req_kind, req, oncomplete, 2, call_argv);
}

napi_value FsUtimes(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "utimes", req_kind, req, oncomplete, 3, call_argv);
}

napi_value FsFutimes(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "futimes", req_kind, req, oncomplete, 3, call_argv);
}

napi_value FsLutimes(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, "lutimes", req_kind, req, oncomplete, 3, call_argv);
}

napi_value FsRead(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 6 ? argv[5] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  size_t offset = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    uint32_t offset_u32 = 0;
    napi_get_value_uint32(env, argv[2], &offset_u32);
    offset = offset_u32;
  }
  size_t length = ByteLengthOfValue(env, argc >= 2 ? argv[1] : nullptr);
  if (argc >= 4 && argv[3] != nullptr) {
    uint32_t length_u32 = 0;
    napi_get_value_uint32(env, argv[3], &length_u32);
    length = length_u32;
  } else if (length >= offset) {
    length -= offset;
  }
  const int64_t position = GetInt64OrDefault(env, argc >= 5 ? argv[4] : nullptr, -1);

  if (req_kind != ReqKind::kNone) {
    if (length == 0) {
      napi_value zero = nullptr;
      napi_create_int64(env, 0, &zero);
      if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, zero);
      CompleteReqWithExtra(env, req_kind, req, oncomplete, nullptr, zero, argc >= 2 ? argv[1] : nullptr);
      return req_kind == ReqKind::kCallback ? Undefined(env) : zero;
    }

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "read";
      async_req->result_kind = AsyncFsResultKind::kInt64;
      async_req->hold_refs = new napi_ref[1]();
      async_req->hold_ref_count = 1;
      async_req->bufs = new uv_buf_t[1];
      async_req->nbufs = 1;

      napi_value hold_value = nullptr;
      const bool extra_ok = req_kind != ReqKind::kCallback ||
                            argv[1] == nullptr ||
                            napi_create_reference(env, argv[1], 1, &async_req->extra_ref) == napi_ok;
      if (ExtractByteSpanForAsyncIo(env, argc >= 2 ? argv[1] : nullptr, offset, length, &hold_value, &async_req->bufs[0]) &&
          extra_ok &&
          hold_value != nullptr &&
          napi_create_reference(env, hold_value, 1, &async_req->hold_refs[0]) == napi_ok) {
        uv_loop_t* loop = EdgeGetEnvLoop(env);
        const int rc = loop != nullptr
                           ? uv_fs_read(loop,
                                        &async_req->req,
                                        fd,
                                        async_req->bufs,
                                        1,
                                        position,
                                        AfterAsyncFsReq)
                           : UV_EINVAL;
        if (rc < 0) FinishAsyncFsReq(async_req, rc);
        return req_kind == ReqKind::kPromise ? promise : Undefined(env);
      }

      FinishAsyncFsReq(async_req, UV_EINVAL);
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  if (length == 0) {
    napi_value zero = nullptr;
    napi_create_int64(env, 0, &zero);
    return zero != nullptr ? zero : Undefined(env);
  }

  napi_value hold_value = nullptr;
  uv_buf_t buf = uv_buf_init(nullptr, 0);
  if (!ExtractByteSpanForAsyncIo(env, argc >= 2 ? argv[1] : nullptr, offset, length, &hold_value, &buf)) {
    napi_throw(env, CreateUvExceptionValue(env, UV_EINVAL, "read"));
    return nullptr;
  }

  uv_fs_t uv_req{};
  const int result = uv_fs_read(nullptr, &uv_req, fd, &buf, 1, position, nullptr);
  uv_fs_req_cleanup(&uv_req);
  if (result < 0) {
    napi_throw(env, CreateUvExceptionValue(env, result, "read"));
    return nullptr;
  }

  napi_value out = nullptr;
  napi_create_int64(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FsOpen(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  if (req_kind != ReqKind::kNone) {
    std::string path;
    if (!ValueToPathString(env, argc >= 1 ? argv[0] : nullptr, &path)) {
      napi_value err = CreateUvExceptionValue(env, UV_EINVAL, "open");
      if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
      CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
      return Undefined(env);
    }

    int32_t flags = 0;
    int32_t mode = 0;
    if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &flags);
    if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &mode);

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "open";
      async_req->result_kind = AsyncFsResultKind::kInt64;
      async_req->track_unmanaged_fd = true;
      async_req->path_storage = std::move(path);
      async_req->open_flags = flags;
      async_req->is_open_req = true;

      uv_loop_t* loop = EdgeGetEnvLoop(env);
      async_req->uses_uv_fs_req = true;
      const int rc = loop != nullptr ? uv_fs_open(loop,
                                                  &async_req->req,
                                                  async_req->path_storage.c_str(),
                                                  flags,
                                                  mode,
                                                  AfterAsyncFsReq)
                                     : UV_EINVAL;
      if (rc < 0) FinishAsyncFsReq(async_req, rc);
      if (rc >= 0 && FsOpenAccessMode(flags) != UV_FS_O_WRONLY) {
        ReleasePendingFifoWriterOpens(env, async_req->path_storage);
      }
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  napi_value out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "open", 3, call_argv, &out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  EdgeWorkerEnvAddUnmanagedFd(env, static_cast<int>(GetInt64OrDefault(env, out, -1)));
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
    return Undefined(env);
  }
  return out;
}

napi_value FsRealpath(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  std::string encoding = "utf8";
  if (argc >= 2 && argv[1] != nullptr) ValueToUtf8(env, argv[1], &encoding);

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value raw_out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "realpath", 1, call_argv, &raw_out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  napi_value out = raw_out;
  if (encoding == "buffer") out = BufferFromValue(env, raw_out, nullptr);
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value FsClose(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;

  napi_value req = argc >= 2 ? argv[1] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  EdgeWorkerEnvRemoveUnmanagedFd(env, fd);

  if (req_kind != ReqKind::kNone) {
    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "close";
      async_req->uses_uv_fs_req = true;

      uv_loop_t* loop = EdgeGetEnvLoop(env);
      const int rc = loop != nullptr ? uv_fs_close(loop, &async_req->req, fd, AfterAsyncFsReq) : UV_EINVAL;
      if (rc < 0) FinishAsyncFsReq(async_req, rc);
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "close", 1, call_argv, &out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  if (req_kind == ReqKind::kCallback) {
    CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value FsReadBuffers(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  int64_t position = -1;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int64(env, argv[2], &position);

  uint32_t len = 0;
  bool is_array = false;
  if (argc < 2 || napi_is_array(env, argv[1], &is_array) != napi_ok || !is_array ||
      napi_get_array_length(env, argv[1], &len) != napi_ok) {
    return Undefined(env);
  }

  int64_t total = 0;
  for (uint32_t i = 0; i < len; ++i) {
    napi_value view = nullptr;
    if (napi_get_element(env, argv[1], i, &view) != napi_ok || view == nullptr) continue;
    const napi_value read_argv[5] = {argv[0], view, nullptr, nullptr, nullptr};
    napi_value offset = nullptr;
    napi_value length = nullptr;
    napi_create_uint32(env, 0, &offset);
    size_t chunk_len = ByteLengthOfValue(env, view);
    napi_create_uint32(env, static_cast<uint32_t>(chunk_len), &length);
    napi_value pos = nullptr;
    napi_create_int64(env, position, &pos);
    napi_value mutable_args[5] = {argv[0], view, offset, length, pos};
    napi_value chunk_out = nullptr;
    napi_value err = nullptr;
    if (!CallRaw(env, "readSync", 5, mutable_args, &chunk_out, &err)) {
      if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
      CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
      if (req_kind == ReqKind::kCallback) return Undefined(env);
      if (err != nullptr) {
        napi_throw(env, err);
        return nullptr;
      }
      return Undefined(env);
    }
    int64_t n = 0;
    napi_get_value_int64(env, chunk_out, &n);
    total += n;
    if (position >= 0 && n > 0) position += n;
  }

  napi_value out = nullptr;
  napi_create_int64(env, total, &out);
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, out);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, out);
  return out;
}

napi_value FsWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t validated_fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &validated_fd)) return nullptr;
  napi_value req = argc >= 6 ? argv[5] : nullptr;
  napi_value ctx = argc >= 7 ? argv[6] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  if (req_kind != ReqKind::kNone) {
    int32_t fd = validated_fd;
    size_t offset = 0;
    if (argc >= 3 && argv[2] != nullptr) {
      uint32_t offset_u32 = 0;
      napi_get_value_uint32(env, argv[2], &offset_u32);
      offset = offset_u32;
    }
    size_t length = ByteLengthOfValue(env, argc >= 2 ? argv[1] : nullptr);
    if (argc >= 4 && argv[3] != nullptr) {
      uint32_t length_u32 = 0;
      napi_get_value_uint32(env, argv[3], &length_u32);
      length = length_u32;
    } else if (length >= offset) {
      length -= offset;
    }
    const int64_t position = GetInt64OrDefault(env, argc >= 5 ? argv[4] : nullptr, -1);
    if (length == 0) {
      napi_value zero = nullptr;
      napi_create_int64(env, 0, &zero);
      if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, zero);
      CompleteReqWithExtra(env, req_kind, req, oncomplete, nullptr, zero, argc >= 2 ? argv[1] : nullptr);
      return req_kind == ReqKind::kCallback ? Undefined(env) : zero;
    }

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "write";
      async_req->result_kind = AsyncFsResultKind::kInt64;
      async_req->hold_refs = new napi_ref[1]();
      async_req->hold_ref_count = 1;
      async_req->bufs = new uv_buf_t[1];
      async_req->nbufs = 1;

      napi_value hold_value = nullptr;
      const bool extra_ok = req_kind != ReqKind::kCallback ||
                            argv[1] == nullptr ||
                            napi_create_reference(env, argv[1], 1, &async_req->extra_ref) == napi_ok;
      if (ExtractByteSpanForAsyncIo(env, argc >= 2 ? argv[1] : nullptr, offset, length, &hold_value, &async_req->bufs[0]) &&
          extra_ok &&
          hold_value != nullptr &&
          napi_create_reference(env, hold_value, 1, &async_req->hold_refs[0]) == napi_ok) {
        uv_loop_t* loop = EdgeGetEnvLoop(env);
        const int rc = loop != nullptr
                           ? uv_fs_write(loop,
                                         &async_req->req,
                                         fd,
                                         async_req->bufs,
                                         1,
                                         position,
                                         AfterAsyncFsReq)
                           : UV_EINVAL;
        if (rc < 0) FinishAsyncFsReq(async_req, rc);
        return req_kind == ReqKind::kPromise ? promise : Undefined(env);
      }

      FinishAsyncFsReq(async_req, UV_EINVAL);
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  napi_value out = nullptr;
  napi_value hold_value = nullptr;
  uv_buf_t buf = uv_buf_init(nullptr, 0);
  size_t offset = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    uint32_t offset_u32 = 0;
    napi_get_value_uint32(env, argv[2], &offset_u32);
    offset = offset_u32;
  }
  size_t length = ByteLengthOfValue(env, argc >= 2 ? argv[1] : nullptr);
  if (argc >= 4 && argv[3] != nullptr) {
    uint32_t length_u32 = 0;
    napi_get_value_uint32(env, argv[3], &length_u32);
    length = length_u32;
  } else if (length >= offset) {
    length -= offset;
  }
  const int64_t position = GetInt64OrDefault(env, argc >= 5 ? argv[4] : nullptr, -1);
  if (length == 0) {
    napi_value zero = nullptr;
    napi_create_int64(env, 0, &zero);
    return zero != nullptr ? zero : Undefined(env);
  }
  if (!ExtractByteSpanForAsyncIo(env, argc >= 2 ? argv[1] : nullptr, offset, length, &hold_value, &buf)) {
    napi_throw(env, CreateUvExceptionValue(env, UV_EINVAL, "write"));
    return nullptr;
  }
  if (!SyncWriteWithUv(env, validated_fd, &buf, 1, position, ctx, &out)) {
    return (ctx != nullptr && !IsUndefined(env, ctx)) ? Undefined(env) : nullptr;
  }
  return out;
}

napi_value FsWriteString(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t validated_fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &validated_fd)) return nullptr;
  napi_value req = argc >= 5 ? argv[4] : nullptr;
  napi_value ctx = argc >= 6 ? argv[5] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  std::string encoding = "utf8";
  if (argc >= 4 && argv[3] != nullptr) ValueToUtf8(env, argv[3], &encoding);
  napi_value buffer = BufferFromValue(env, argc >= 2 ? argv[1] : Undefined(env), encoding.c_str());
  const size_t byte_length = ByteLengthOfValue(env, buffer);
  if (req_kind != ReqKind::kNone) {
    const int32_t fd = validated_fd;
    const int64_t position = GetInt64OrDefault(env, argc >= 3 ? argv[2] : nullptr, -1);
    if (byte_length == 0) {
      napi_value zero = nullptr;
      napi_create_int64(env, 0, &zero);
      if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, zero);
      CompleteReqWithExtra(env, req_kind, req, oncomplete, nullptr, zero, argc >= 2 ? argv[1] : nullptr);
      return req_kind == ReqKind::kCallback ? Undefined(env) : zero;
    }

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "write";
      async_req->result_kind = AsyncFsResultKind::kInt64;
      async_req->hold_refs = new napi_ref[1]();
      async_req->hold_ref_count = 1;
      async_req->bufs = new uv_buf_t[1];
      async_req->nbufs = 1;

      napi_value hold_value = nullptr;
      const bool extra_ok = req_kind != ReqKind::kCallback ||
                            argv[1] == nullptr ||
                            napi_create_reference(env, argv[1], 1, &async_req->extra_ref) == napi_ok;
      if (ExtractByteSpanForAsyncIo(env, buffer, 0, byte_length, &hold_value, &async_req->bufs[0]) &&
          extra_ok &&
          hold_value != nullptr &&
          napi_create_reference(env, hold_value, 1, &async_req->hold_refs[0]) == napi_ok) {
        uv_loop_t* loop = EdgeGetEnvLoop(env);
        const int rc = loop != nullptr
                           ? uv_fs_write(loop,
                                         &async_req->req,
                                         fd,
                                         async_req->bufs,
                                         1,
                                         position,
                                         AfterAsyncFsReq)
                           : UV_EINVAL;
        if (rc < 0) FinishAsyncFsReq(async_req, rc);
        return req_kind == ReqKind::kPromise ? promise : Undefined(env);
      }

      FinishAsyncFsReq(async_req, UV_EINVAL);
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  napi_value out = nullptr;
  if (byte_length == 0) {
    napi_create_int64(env, 0, &out);
    return out != nullptr ? out : Undefined(env);
  }
  napi_value hold_value = nullptr;
  uv_buf_t buf = uv_buf_init(nullptr, 0);
  if (!ExtractByteSpanForAsyncIo(env, buffer, 0, byte_length, &hold_value, &buf)) {
    napi_throw(env, CreateUvExceptionValue(env, UV_EINVAL, "write"));
    return nullptr;
  }
  const int64_t position = GetInt64OrDefault(env, argc >= 3 ? argv[2] : nullptr, -1);
  if (!SyncWriteWithUv(env, validated_fd, &buf, 1, position, ctx, &out)) {
    return (ctx != nullptr && !IsUndefined(env, ctx)) ? Undefined(env) : nullptr;
  }
  return out;
}

napi_value FsWriteBuffers(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t validated_fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &validated_fd)) return nullptr;
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  int64_t position = -1;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_int64(env, argv[2], &position);

  uint32_t len = 0;
  bool is_array = false;
  if (argc < 2 || napi_is_array(env, argv[1], &is_array) != napi_ok || !is_array ||
      napi_get_array_length(env, argv[1], &len) != napi_ok) {
    return Undefined(env);
  }

  bool all_chunks_empty = true;
  for (uint32_t i = 0; i < len; ++i) {
    napi_value chunk = nullptr;
    if (napi_get_element(env, argv[1], i, &chunk) != napi_ok || chunk == nullptr) {
      all_chunks_empty = false;
      break;
    }
    if (ByteLengthOfValue(env, chunk) != 0) {
      all_chunks_empty = false;
      break;
    }
  }

  if (len == 0 || all_chunks_empty) {
    napi_value zero = nullptr;
    napi_create_int64(env, 0, &zero);
    if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, zero);
    if (req_kind == ReqKind::kCallback) {
      CompleteReqWithExtra(env,
                           req_kind,
                           req,
                           oncomplete,
                           nullptr,
                           zero != nullptr ? zero : Undefined(env),
                           argc >= 2 ? argv[1] : nullptr);
      return Undefined(env);
    }
    return zero != nullptr ? zero : Undefined(env);
  }

  if (req_kind != ReqKind::kNone) {
    const int32_t fd = validated_fd;

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "write";
      async_req->result_kind = AsyncFsResultKind::kInt64;
      async_req->hold_refs = new napi_ref[len]();
      async_req->hold_ref_count = len;
      async_req->bufs = new uv_buf_t[len];
      async_req->nbufs = len;

      const bool extra_ok = req_kind != ReqKind::kCallback ||
                            argv[1] == nullptr ||
                            napi_create_reference(env, argv[1], 1, &async_req->extra_ref) == napi_ok;
      bool ok = true;
      for (uint32_t i = 0; i < len; ++i) {
        napi_value chunk = nullptr;
        if (napi_get_element(env, argv[1], i, &chunk) != napi_ok || chunk == nullptr) {
          ok = false;
          break;
        }
        napi_value hold_value = nullptr;
        const size_t byte_length = ByteLengthOfValue(env, chunk);
        if (!ExtractByteSpanForAsyncIo(env, chunk, 0, byte_length, &hold_value, &async_req->bufs[i]) ||
            hold_value == nullptr ||
            napi_create_reference(env, hold_value, 1, &async_req->hold_refs[i]) != napi_ok) {
          ok = false;
          break;
        }
      }

      if (ok && extra_ok) {
        uv_loop_t* loop = EdgeGetEnvLoop(env);
        const int rc = loop != nullptr
                           ? uv_fs_write(loop,
                                         &async_req->req,
                                         fd,
                                         async_req->bufs,
                                         len,
                                         position,
                                         AfterAsyncFsReq)
                           : UV_EINVAL;
        if (rc < 0) FinishAsyncFsReq(async_req, rc);
        return req_kind == ReqKind::kPromise ? promise : Undefined(env);
      }

      FinishAsyncFsReq(async_req, UV_EINVAL);
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  std::vector<napi_value> hold_values(len);
  std::vector<uv_buf_t> bufs(len);
  for (uint32_t i = 0; i < len; ++i) {
    napi_value chunk = nullptr;
    if (napi_get_element(env, argv[1], i, &chunk) != napi_ok || chunk == nullptr) {
      napi_throw(env, CreateUvExceptionValue(env, UV_EINVAL, "write"));
      return nullptr;
    }
    const size_t chunk_len = ByteLengthOfValue(env, chunk);
    if (!ExtractByteSpanForAsyncIo(env, chunk, 0, chunk_len, &hold_values[i], &bufs[i])) {
      napi_throw(env, CreateUvExceptionValue(env, UV_EINVAL, "write"));
      return nullptr;
    }
  }

  napi_value out = nullptr;
  if (!SyncWriteWithUv(env, validated_fd, bufs.data(), bufs.size(), position, nullptr, &out)) {
    return nullptr;
  }
  return out;
}

napi_value FsOpenFileHandle(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);

  if (req_kind != ReqKind::kNone) {
    std::string path;
    int32_t flags = 0;
    int32_t mode = 0;
    if (!ValueToUtf8(env, argc >= 1 ? argv[0] : nullptr, &path)) {
      napi_value err = CreateUvExceptionValue(env, UV_EINVAL, "open");
      if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
      CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
      return Undefined(env);
    }
    if (argc >= 2 && argv[1] != nullptr) napi_get_value_int32(env, argv[1], &flags);
    if (argc >= 3 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &mode);

    napi_value promise = nullptr;
    AsyncFsReq* async_req = CreateAsyncFsReq(env, req_kind, req, oncomplete, &promise);
    if (async_req != nullptr) {
      async_req->syscall = "open";
      async_req->result_kind = AsyncFsResultKind::kFileHandle;
      async_req->path_storage = std::move(path);
      async_req->open_flags = flags;
      async_req->is_open_req = true;
      uv_loop_t* loop = EdgeGetEnvLoop(env);
      async_req->uses_uv_fs_req = true;
      const int rc = loop != nullptr ? uv_fs_open(loop,
                                                  &async_req->req,
                                                  async_req->path_storage.c_str(),
                                                  flags,
                                                  mode,
                                                  AfterAsyncFsReq)
                                     : UV_EINVAL;
      if (rc < 0) FinishAsyncFsReq(async_req, rc);
      if (rc >= 0 && FsOpenAccessMode(flags) != UV_FS_O_WRONLY) {
        ReleasePendingFifoWriterOpens(env, async_req->path_storage);
      }
      return req_kind == ReqKind::kPromise ? promise : Undefined(env);
    }
  }

  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  napi_value fd_value = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "open", 3, call_argv, &fd_value, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }

  FsBindingState* st = GetState(env);
  napi_value ctor = st == nullptr ? nullptr : GetRefValue(env, st->file_handle_ctor_ref);
  if (ctor == nullptr) return Undefined(env);
  napi_value ctor_argv[1] = {fd_value};
  napi_value handle = nullptr;
  if (napi_new_instance(env, ctor, 1, ctor_argv, &handle) != napi_ok || handle == nullptr) {
    return Undefined(env);
  }

  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, handle);
  CompleteReq(env, req_kind, req, oncomplete, nullptr, handle);
  return handle;
}

napi_value FsReadFileUtf8(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t fd = -1;
  bool is_fd = false;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_number) {
      is_fd = ValidateFdArg(env, argv[0], &fd);
      if (!is_fd) return nullptr;
    }
  }

  if (is_fd) {
    std::string content;
    std::vector<char> chunk(8192);
    while (true) {
      uv_buf_t buf = uv_buf_init(chunk.data(), static_cast<unsigned int>(chunk.size()));
      uv_fs_t req;
      const int rc = uv_fs_read(nullptr, &req, fd, &buf, 1, -1, nullptr);
      const int result = rc < 0 ? rc : static_cast<int>(req.result);
      uv_fs_req_cleanup(&req);
      if (result < 0) {
        napi_throw(env, CreateUvExceptionValue(env, result, "read"));
        return nullptr;
      }
      if (result == 0) break;
      content.append(chunk.data(), static_cast<size_t>(result));
    }

    napi_value out = nullptr;
    napi_create_string_utf8(env, content.c_str(), content.size(), &out);
    return out != nullptr ? out : Undefined(env);
  }

  napi_value out = nullptr;
  napi_value err = nullptr;
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  if (!CallRaw(env, "readFileUtf8", 2, call_argv, &out, &err)) {
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value FsInternalModuleStat(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string path;
  if (argc < 1 || !ValueToUtf8(env, argv[0], &path)) return Undefined(env);

  std::error_code ec;
  auto status = std::filesystem::status(path, ec);
  if (ec) {
    const std::filesystem::path resolved = ResolveSymlinkComponentsForModuleResolution(path);
    if (resolved != std::filesystem::path(path).lexically_normal()) {
      ec.clear();
      status = std::filesystem::status(resolved, ec);
    }
  }
  int32_t out_value = -1;
  if (!ec) {
    if (std::filesystem::is_directory(status)) out_value = 1;
    else if (std::filesystem::is_regular_file(status)) out_value = 0;
    else out_value = 0;
  } else {
    out_value = (ec == std::errc::no_such_file_or_directory || ec == std::errc::not_a_directory) ? -2 : -1;
  }
  napi_value out = nullptr;
  napi_create_int32(env, out_value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FsLegacyMainResolve(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string package_path;
  if (argc < 1 || !ValueToUtf8(env, argv[0], &package_path)) return Undefined(env);

  if (argc < 3 || argv[2] == nullptr) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"base\" argument must be of type string or an instance of URL.");
    return nullptr;
  }
  napi_valuetype base_type = napi_undefined;
  if (napi_typeof(env, argv[2], &base_type) != napi_ok || base_type != napi_string) {
    napi_throw_type_error(env,
                          "ERR_INVALID_ARG_TYPE",
                          "The \"base\" argument must be of type string or an instance of URL.");
    return nullptr;
  }

  static const char* ext[] = {"", ".js", ".json", ".node", "/index.js", "/index.json", "/index.node",
                              ".js", ".json", ".node"};

  auto internal_module_stat = [&](const std::string& candidate) -> int32_t {
    std::error_code ec;
    const auto status = std::filesystem::symlink_status(candidate, ec);
    if (ec) return -2;
    return std::filesystem::is_directory(status) ? 1 : 0;
  };

  std::string package_main;
  if (argc >= 2 && argv[1] != nullptr) ValueToUtf8(env, argv[1], &package_main);

  if (!package_main.empty()) {
    const std::string initial =
        edge_path::FromNamespacedPath(edge_path::PathResolve({package_path, package_main}));
    for (int i = 0; i < 7; ++i) {
      if (internal_module_stat(initial + ext[i]) == 0) {
        napi_value out = nullptr;
        napi_create_int32(env, i, &out);
        return out != nullptr ? out : Undefined(env);
      }
    }
  }

  const std::string fallback =
      edge_path::FromNamespacedPath(edge_path::PathResolve({package_path, "./index"}));
  for (int i = 7; i < 10; ++i) {
    if (internal_module_stat(fallback + ext[i]) == 0) {
      napi_value out = nullptr;
      napi_create_int32(env, i, &out);
      return out != nullptr ? out : Undefined(env);
    }
  }

  std::string base_string;
  ValueToUtf8(env, argv[2], &base_string);
  auto base_url = ada::parse<ada::url_aggregator>(base_string);
  if (!base_url) {
    napi_throw_error(env, "ERR_INVALID_URL", "Invalid URL");
    return nullptr;
  }

  const std::string module_base = edge_path::NormalizeFileURLOrPath(base_string);
  const std::string missing = package_main.empty() ? fallback + ".js" : package_path + "/" + package_main;
  napi_throw_error(env,
                   "ERR_MODULE_NOT_FOUND",
                   ("Cannot find package '" + missing + "' imported from " + module_base).c_str());
  return nullptr;
}

bool ThrowCpError(napi_env env, const char* code, const std::string& message) {
  napi_throw_error(env, code, message.c_str());
  return false;
}

int ErrnoFromStdError(const std::error_code& error_code) {
#ifdef _WIN32
  return uv_translate_sys_error(error_code.value());
#else
  return error_code.value() > 0 ? -error_code.value() : error_code.value();
#endif
}

bool ThrowStdFsError(napi_env env,
                     const std::error_code& error_code,
                     const char* syscall,
                     const std::string& path = std::string()) {
  napi_value err = CreateUvExceptionValue(env, ErrnoFromStdError(error_code), syscall);
  if (err == nullptr) {
    napi_throw_error(env, nullptr, error_code.message().c_str());
    return false;
  }
  if (!path.empty()) {
    napi_value path_value = nullptr;
    if (napi_create_string_utf8(env, path.c_str(), path.size(), &path_value) == napi_ok && path_value != nullptr) {
      napi_set_named_property(env, err, "path", path_value);
    }
  }
  napi_throw(env, err);
  return false;
}

std::string FsPathToString(const std::filesystem::path& path) {
  return path.lexically_normal().string();
}

bool CopyUtimesForCp(napi_env env, const std::filesystem::path& src, const std::filesystem::path& dest) {
  uv_fs_t req;
  const std::string src_path = FsPathToString(src);
  int result = uv_fs_stat(nullptr, &req, src_path.c_str(), nullptr);
  if (result < 0) {
    uv_fs_req_cleanup(&req);
    napi_throw(env, CreateUvExceptionValue(env, result, "stat"));
    return false;
  }

  const uv_stat_t* stat = static_cast<const uv_stat_t*>(req.ptr);
  const double source_atime = stat->st_atim.tv_sec + stat->st_atim.tv_nsec / 1e9;
  const double source_mtime = stat->st_mtim.tv_sec + stat->st_mtim.tv_nsec / 1e9;
  uv_fs_req_cleanup(&req);

  const std::string dest_path = FsPathToString(dest);
  result = uv_fs_utime(nullptr, &req, dest_path.c_str(), source_atime, source_mtime, nullptr);
  uv_fs_req_cleanup(&req);
  if (result < 0) {
    napi_throw(env, CreateUvExceptionValue(env, result, "utime"));
    return false;
  }
  return true;
}

std::vector<std::string> NormalizePathToArray(const std::filesystem::path& path) {
  std::vector<std::string> parts;
  std::filesystem::path abs = std::filesystem::absolute(path);
  for (const auto& part : abs) {
    if (!part.empty()) parts.push_back(part.string());
  }
  return parts;
}

bool IsInsideDir(const std::filesystem::path& src, const std::filesystem::path& dest) {
  const auto src_parts = NormalizePathToArray(src);
  const auto dest_parts = NormalizePathToArray(dest);
  if (src_parts.size() > dest_parts.size()) return false;
  return std::equal(src_parts.begin(), src_parts.end(), dest_parts.begin());
}

napi_value FsCpSyncCheckPaths(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string src;
  std::string dest;
  if (argc < 2 || !ValueToUtf8(env, argv[0], &src) || !ValueToUtf8(env, argv[1], &dest)) return Undefined(env);
  const std::filesystem::path src_path(src);
  const std::filesystem::path dest_path(dest);
  bool dereference = false;
  bool recursive = false;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &dereference);
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_bool(env, argv[3], &recursive);

  std::error_code ec;
  const auto src_status = dereference ? std::filesystem::status(src_path, ec)
                                      : std::filesystem::symlink_status(src_path, ec);
  if (ec) {
    napi_throw(env,
               CreateUvExceptionValue(env, ErrnoFromStdError(ec), dereference ? "stat" : "lstat"));
    return nullptr;
  }

  ec.clear();
  const auto dest_status = dereference ? std::filesystem::status(dest_path, ec)
                                       : std::filesystem::symlink_status(dest_path, ec);
  const bool dest_exists = !ec && dest_status.type() != std::filesystem::file_type::not_found;
  const bool src_is_dir = src_status.type() == std::filesystem::file_type::directory;

  const std::string src_path_str = FsPathToString(src_path);
  const std::string dest_path_str = FsPathToString(dest_path);

  if (!ec) {
    std::error_code equivalent_error;
    if (std::filesystem::equivalent(src_path, dest_path, equivalent_error)) {
      ThrowCpError(env, "ERR_FS_CP_EINVAL", "src and dest cannot be the same " + dest_path_str);
      return nullptr;
    }

    const bool dest_is_dir = dest_status.type() == std::filesystem::file_type::directory;
    if (src_is_dir && !dest_is_dir) {
      ThrowCpError(env,
                   "ERR_FS_CP_DIR_TO_NON_DIR",
                   "Cannot overwrite non-directory " + dest_path_str + " with directory " + src_path_str);
      return nullptr;
    }
    if (!src_is_dir && dest_is_dir) {
      ThrowCpError(env,
                   "ERR_FS_CP_NON_DIR_TO_DIR",
                   "Cannot overwrite directory " + dest_path_str + " with non-directory " + src_path_str);
      return nullptr;
    }
  } else {
    ec.clear();
  }

  if (src_is_dir && IsInsideDir(src_path, dest_path) && src_path != dest_path) {
    ThrowCpError(env,
                 "ERR_FS_CP_EINVAL",
                 "Cannot copy " + src_path_str + " to a subdirectory of self " + dest_path_str);
    return nullptr;
  }

  auto dest_parent = dest_path.parent_path();
  while (src_path.parent_path() != dest_parent &&
         dest_parent.has_parent_path() &&
         dest_parent.parent_path() != dest_parent) {
    std::error_code equivalent_error;
    if (std::filesystem::equivalent(src_path, dest_parent, equivalent_error)) {
      ThrowCpError(env,
                   "ERR_FS_CP_EINVAL",
                   "Cannot copy " + src_path_str + " to a subdirectory of self " + dest_path_str);
      return nullptr;
    }
    if (equivalent_error) break;
    dest_parent = dest_parent.parent_path();
  }

  if (src_is_dir && !recursive) {
    ThrowCpError(env,
                 "ERR_FS_EISDIR",
                 "Recursive option not enabled, cannot copy a directory: " + src_path_str);
    return nullptr;
  }

  switch (src_status.type()) {
    case std::filesystem::file_type::socket:
      ThrowCpError(env, "ERR_FS_CP_SOCKET", "Cannot copy a socket file: " + dest_path_str);
      return nullptr;
    case std::filesystem::file_type::fifo:
      ThrowCpError(env, "ERR_FS_CP_FIFO_PIPE", "Cannot copy a FIFO pipe: " + dest_path_str);
      return nullptr;
    case std::filesystem::file_type::unknown:
      ThrowCpError(env, "ERR_FS_CP_UNKNOWN", "Cannot copy an unknown file type: " + dest_path_str);
      return nullptr;
    default:
      break;
  }

  const auto parent_path = dest_path.parent_path();
  if ((!dest_exists || !std::filesystem::exists(parent_path)) && !parent_path.empty()) {
    std::filesystem::create_directories(parent_path, ec);
    if (ec) return ThrowStdFsError(env, ec, "cp", FsPathToString(parent_path)), nullptr;
  }
  return Undefined(env);
}

napi_value FsCpSyncOverrideFile(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string src;
  std::string dest;
  if (argc < 2 || !ValueToUtf8(env, argv[0], &src) || !ValueToUtf8(env, argv[1], &dest)) return Undefined(env);
  int32_t mode = 0;
  if (argc >= 3 && argv[2] != nullptr && !ValidateCopyFileModeArg(env, argv[2], &mode)) return nullptr;
  bool preserve_timestamps = false;
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_bool(env, argv[3], &preserve_timestamps);

  const std::filesystem::path src_path(src);
  const std::filesystem::path dest_path(dest);
  std::error_code ec;
  if (!std::filesystem::remove(dest_path, ec) && ec) return ThrowStdFsError(env, ec, "unlink", dest), nullptr;

  if (mode == 0) {
    ec.clear();
    if (!std::filesystem::copy_file(src_path, dest_path, ec)) {
      if (ec) return ThrowStdFsError(env, ec, "cp", dest), nullptr;
    }
  } else {
    uv_fs_t req;
    const int result = uv_fs_copyfile(nullptr, &req, src.c_str(), dest.c_str(), mode, nullptr);
    uv_fs_req_cleanup(&req);
    if (result < 0) {
      napi_throw(env, CreateUvExceptionValue(env, result, "cp"));
      return nullptr;
    }
  }

  if (preserve_timestamps && !CopyUtimesForCp(env, src_path, dest_path)) return nullptr;
  return Undefined(env);
}

napi_value FsCpSyncCopyDir(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::string src;
  std::string dest;
  if (argc < 2 || !ValueToUtf8(env, argv[0], &src) || !ValueToUtf8(env, argv[1], &dest)) return Undefined(env);
  const std::filesystem::path src_path(src);
  const std::filesystem::path dest_path(dest);
  bool force = false;
  bool dereference = false;
  bool error_on_exist = false;
  bool verbatim_symlinks = false;
  bool preserve_timestamps = false;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &force);
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_bool(env, argv[3], &dereference);
  if (argc >= 5 && argv[4] != nullptr) napi_get_value_bool(env, argv[4], &error_on_exist);
  if (argc >= 6 && argv[5] != nullptr) napi_get_value_bool(env, argv[5], &verbatim_symlinks);
  if (argc >= 7 && argv[6] != nullptr) napi_get_value_bool(env, argv[6], &preserve_timestamps);

  std::error_code ec;
  std::filesystem::create_directories(dest_path, ec);
  if (ec) return ThrowStdFsError(env, ec, "cp", dest), nullptr;

  auto file_copy_opts = std::filesystem::copy_options::none;
  if (force) file_copy_opts = std::filesystem::copy_options::overwrite_existing;
  else if (!error_on_exist) file_copy_opts = std::filesystem::copy_options::skip_existing;

  std::function<bool(const std::filesystem::path&, const std::filesystem::path&)> copy_dir_contents;
  copy_dir_contents = [&](const std::filesystem::path& current_src, const std::filesystem::path& current_dest) {
    std::error_code iter_error;
    for (const auto& entry : std::filesystem::directory_iterator(current_src, iter_error)) {
      if (iter_error) return ThrowStdFsError(env, iter_error, "cp", FsPathToString(current_dest));
      const auto dest_file_path = current_dest / entry.path().filename();
      const std::string dest_str = FsPathToString(current_dest);

      if (entry.is_symlink()) {
        if (verbatim_symlinks) {
          std::filesystem::copy_symlink(entry.path(), dest_file_path, iter_error);
          if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);
        } else {
          auto symlink_target = std::filesystem::read_symlink(entry.path(), iter_error);
          if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);

          if (std::filesystem::exists(dest_file_path)) {
            if (std::filesystem::is_symlink(dest_file_path)) {
              auto current_dest_symlink_target = std::filesystem::read_symlink(dest_file_path, iter_error);
              if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);

              if (!dereference &&
                  std::filesystem::is_directory(symlink_target) &&
                  IsInsideDir(symlink_target, current_dest_symlink_target)) {
                return ThrowCpError(env,
                                    "ERR_FS_CP_EINVAL",
                                    "Cannot copy " + FsPathToString(symlink_target) +
                                        " to a subdirectory of self " +
                                        FsPathToString(current_dest_symlink_target));
              }

              if (std::filesystem::is_directory(dest_file_path) &&
                  IsInsideDir(current_dest_symlink_target, symlink_target)) {
                return ThrowCpError(env,
                                    "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY",
                                    "cannot overwrite " + FsPathToString(current_dest_symlink_target) +
                                        " with " + FsPathToString(symlink_target));
              }

              std::filesystem::remove(dest_file_path, iter_error);
              if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);
            } else if (std::filesystem::is_regular_file(dest_file_path)) {
              if (!dereference || (!force && error_on_exist)) {
                return ThrowCpError(env,
                                    "EEXIST",
                                    "EEXIST: file already exists, cp '" + FsPathToString(dest_file_path) + "'");
              }
            }
          }

          std::filesystem::path target_path = symlink_target;
          if (!target_path.is_absolute()) {
            target_path = std::filesystem::weakly_canonical(entry.path().parent_path() / target_path);
          }
          if (entry.is_directory()) {
            std::filesystem::create_directory_symlink(target_path, dest_file_path, iter_error);
          } else {
            std::filesystem::create_symlink(target_path, dest_file_path, iter_error);
          }
          if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);
        }
      } else if (entry.is_directory()) {
        std::filesystem::create_directory(dest_file_path, iter_error);
        if (iter_error) return ThrowStdFsError(env, iter_error, "cp", dest_str);
        if (!copy_dir_contents(entry.path(), dest_file_path)) return false;
      } else if (entry.is_regular_file()) {
        std::filesystem::copy_file(entry.path(), dest_file_path, file_copy_opts, iter_error);
        if (iter_error) {
          if (iter_error.value() == EEXIST) {
            return ThrowCpError(env,
                                "ERR_FS_CP_EEXIST",
                                "[ERR_FS_CP_EEXIST]: Target already exists: cp returned EEXIST (" +
                                    FsPathToString(dest_file_path) + " already exists)");
          }
          return ThrowStdFsError(env, iter_error, "cp", dest_str);
        }
        if (preserve_timestamps && !CopyUtimesForCp(env, entry.path(), dest_file_path)) return false;
      }
    }
    return true;
  };

  if (!copy_dir_contents(src_path, dest_path)) return nullptr;
  return Undefined(env);
}

napi_value FsLink(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 3 ? argv[2] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[2] = {argc >= 1 ? argv[0] : Undefined(env), argc >= 2 ? argv[1] : Undefined(env)};
  napi_value out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "link", 2, call_argv, &out, &err)) {
    if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
    CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
    if (req_kind == ReqKind::kCallback) return Undefined(env);
    if (err != nullptr) {
      napi_throw(env, err);
      return nullptr;
    }
    return Undefined(env);
  }
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
  return Undefined(env);
}

napi_value FsFdatasync(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  napi_value req = argc >= 2 ? argv[1] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[1] = {argc >= 1 ? argv[0] : Undefined(env)};
  napi_value out = nullptr;
  napi_value err = nullptr;
  if (!CallRaw(env, "fdatasync", 1, call_argv, &out, &err)) {
    if (!CallRaw(env, "fsync", 1, call_argv, &out, &err)) {
      if (req_kind == ReqKind::kPromise) return MakeRejectedPromise(env, err);
      CompleteReq(env, req_kind, req, oncomplete, err, Undefined(env));
      if (req_kind == ReqKind::kCallback) return Undefined(env);
      if (err != nullptr) {
        napi_throw(env, err);
        return nullptr;
      }
      return Undefined(env);
    }
  }
  if (req_kind == ReqKind::kPromise) return MakeResolvedPromise(env, Undefined(env));
  CompleteReq(env, req_kind, req, oncomplete, nullptr, Undefined(env));
  return Undefined(env);
}

napi_value FsChownCommon(napi_env env, napi_callback_info info, const char* method_name) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  napi_value req = argc >= 4 ? argv[3] : nullptr;
  napi_value oncomplete = nullptr;
  ReqKind req_kind = ParseReq(env, req, &oncomplete);
  napi_value call_argv[3] = {argc >= 1 ? argv[0] : Undefined(env),
                             argc >= 2 ? argv[1] : Undefined(env),
                             argc >= 3 ? argv[2] : Undefined(env)};
  return CompleteVoidRawFsMethod(env, method_name, req_kind, req, oncomplete, 3, call_argv);
}

napi_value FsChown(napi_env env, napi_callback_info info) {
  return FsChownCommon(env, info, "chown");
}

napi_value FsFchown(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t fd = -1;
  if (!ValidateFdArg(env, argc >= 1 ? argv[0] : nullptr, &fd)) return nullptr;
  return FsChownCommon(env, info, "fchown");
}

napi_value FsLchown(napi_env env, napi_callback_info info) {
  return FsChownCommon(env, info, "lchown");
}

void EnsureTypedArrayProperty(napi_env env,
                              napi_value binding,
                              const char* name,
                              napi_typedarray_type type,
                              size_t length) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  napi_value ab = nullptr;
  void* data = nullptr;
  const size_t byte_length = (type == napi_bigint64_array ? sizeof(int64_t) : sizeof(double)) * length;
  if (napi_create_arraybuffer(env, byte_length, &data, &ab) != napi_ok || ab == nullptr || data == nullptr) return;
  std::memset(data, 0, byte_length);
  napi_value out = nullptr;
  if (napi_create_typedarray(env, type, length, ab, 0, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, binding, name, out);
  }
}

void EnsureClassProperty(napi_env env,
                         napi_value binding,
                         const char* name,
                         napi_callback ctor,
                         const std::vector<napi_property_descriptor>& methods,
                         napi_ref* out_ref) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) == napi_ok && has) {
    if (out_ref != nullptr) {
      napi_value existing = nullptr;
      if (napi_get_named_property(env, binding, name, &existing) == napi_ok && existing != nullptr && *out_ref == nullptr) {
        napi_create_reference(env, existing, 1, out_ref);
      }
    }
    return;
  }
  napi_value cls = nullptr;
  if (napi_define_class(env,
                        name,
                        NAPI_AUTO_LENGTH,
                        ctor,
                        nullptr,
                        methods.size(),
                        methods.data(),
                        &cls) != napi_ok ||
      cls == nullptr) {
    return;
  }
  napi_set_named_property(env, binding, name, cls);
  if (out_ref != nullptr) napi_create_reference(env, cls, 1, out_ref);
}

}  // namespace

napi_value InitFs(napi_env env) {
  napi_value binding = EdgeInstallFsBinding(env);
  if (binding == nullptr || IsUndefined(env, binding)) return Undefined(env);

  auto& state = EnsureState(env);
  if (state.binding_ref == nullptr) {
    napi_create_reference(env, binding, 1, &state.binding_ref);
  }

  // Capture raw methods before overriding.
  const char* raw_names[] = {"open",       "close",     "access",   "accessSync", "readSync",  "writeSync",
                             "writeSyncString", "stat",  "lstat",    "fstat",      "statfs",    "mkdir",
                             "readdir",    "realpath",  "readlink", "rename",     "ftruncate", "rmdir",
                             "unlink",     "symlink",   "copyFile", "chmod",      "fchmod",    "utimes",
                             "futimes",    "lutimes",   "writeFileUtf8", "readFileUtf8", "mkdtemp", "fsync",
                             "fdatasync",  "link", "chown", "fchown", "lchown"};
  for (const char* name : raw_names) CaptureRawMethod(env, &state, binding, name);

  // Constants/symbols.
  if (state.k_use_promises_symbol_ref == nullptr) {
    napi_value symbols_binding = edge::binding_registry::Get(env, "symbols");
    if (symbols_binding != nullptr && !IsUndefined(env, symbols_binding)) {
      napi_value candidate = nullptr;
      if (napi_get_named_property(env, symbols_binding, "fs_use_promises_symbol", &candidate) == napi_ok &&
          candidate != nullptr) {
        napi_create_reference(env, candidate, 1, &state.k_use_promises_symbol_ref);
      }
    }
  }
  bool has_k_use_promises = false;
  if (napi_has_named_property(env, binding, "kUsePromises", &has_k_use_promises) == napi_ok && !has_k_use_promises) {
    napi_value symbol = GetUsePromisesSymbol(env);
    if (symbol != nullptr) napi_set_named_property(env, binding, "kUsePromises", symbol);
  } else if (has_k_use_promises && state.k_use_promises_symbol_ref == nullptr) {
    napi_value symbol = nullptr;
    if (napi_get_named_property(env, binding, "kUsePromises", &symbol) == napi_ok && symbol != nullptr) {
      napi_create_reference(env, symbol, 1, &state.k_use_promises_symbol_ref);
    }
  }

  bool has_fields = false;
  if (napi_has_named_property(env, binding, "kFsStatsFieldsNumber", &has_fields) == napi_ok && !has_fields) {
    SetNamedInt(env, binding, "kFsStatsFieldsNumber", static_cast<int32_t>(kFsStatsLength));
  }
  EnsureTypedArrayProperty(env, binding, "statValues", napi_float64_array, 36);
  EnsureTypedArrayProperty(env, binding, "bigintStatValues", napi_bigint64_array, 36);
  EnsureTypedArrayProperty(env, binding, "statFsValues", napi_float64_array, kFsStatFsLength);
  EnsureTypedArrayProperty(env, binding, "bigintStatFsValues", napi_bigint64_array, kFsStatFsLength);

  EnsureClassProperty(env, binding, "FSReqCallback", FSReqCallbackCtor, {}, &state.fs_req_ctor_ref);

  EnsureClassProperty(env,
                      binding,
                      "StatWatcher",
                      StatWatcherCtor,
                      {
                          {"start", nullptr, StatWatcherStart, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"close", nullptr, StatWatcherClose, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"ref", nullptr, StatWatcherRef, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"unref", nullptr, StatWatcherUnref, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"hasRef", nullptr, StatWatcherHasRef, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"getAsyncId", nullptr, StatWatcherGetAsyncId, nullptr, nullptr, nullptr, napi_default,
                           nullptr},
                      },
                      &state.stat_watcher_ctor_ref);

  EnsureClassProperty(env,
                      binding,
                      "FileHandle",
                      FileHandleCtor,
                      {
                          {"close", nullptr, FileHandleClose, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"releaseFD", nullptr, FileHandleReleaseFD, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"readStart", nullptr, FileHandleReadStart, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"readStop", nullptr, FileHandleReadStop, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"shutdown", nullptr, FileHandleShutdown, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"useUserBuffer", nullptr, FileHandleUseUserBuffer, nullptr, nullptr, nullptr, napi_default,
                           nullptr},
                          {"writev", nullptr, FileHandleWritev, nullptr, nullptr, nullptr, napi_default, nullptr},
                          {"writeBuffer", nullptr, FileHandleWriteBuffer, nullptr, nullptr, nullptr, napi_default,
                           nullptr},
                          {"writeAsciiString", nullptr, FileHandleWriteAsciiString, nullptr, nullptr, nullptr,
                           napi_default, nullptr},
                          {"writeUtf8String", nullptr, FileHandleWriteUtf8String, nullptr, nullptr, nullptr,
                           napi_default, nullptr},
                          {"writeUcs2String", nullptr, FileHandleWriteUcs2String, nullptr, nullptr, nullptr,
                           napi_default, nullptr},
                          {"writeLatin1String", nullptr, FileHandleWriteLatin1String, nullptr, nullptr, nullptr,
                           napi_default, nullptr},
                          {"getAsyncId", nullptr, FileHandleGetAsyncId, nullptr, nullptr, nullptr, napi_default,
                           nullptr},
                          {"isStreamBase", nullptr, nullptr, FileHandleIsStreamBase, nullptr, nullptr, napi_default,
                           nullptr},
                          {"fd", nullptr, nullptr, FileHandleGetFd, nullptr, nullptr, napi_default, nullptr},
                          {"_externalStream", nullptr, nullptr, FileHandleGetExternalStream, nullptr, nullptr,
                           napi_default, nullptr},
                          {"bytesRead", nullptr, nullptr, FileHandleGetBytesRead, nullptr, nullptr, napi_default,
                           nullptr},
                          {"bytesWritten", nullptr, nullptr, FileHandleGetBytesWritten, nullptr, nullptr,
                           napi_default, nullptr},
                          {"onread", nullptr, nullptr, FileHandleGetOnread, FileHandleSetOnread, nullptr,
                           napi_default, nullptr},
                      },
                      &state.file_handle_ctor_ref);

  // Missing API surface.
  SetNamedMethod(env, binding, "open", FsOpen);
  SetNamedMethod(env, binding, "close", FsClose);
  SetNamedMethod(env, binding, "access", FsAccess);
  SetNamedMethod(env, binding, "stat", FsStat);
  SetNamedMethod(env, binding, "lstat", FsLstat);
  SetNamedMethod(env, binding, "fstat", FsFstat);
  SetNamedMethod(env, binding, "statfs", FsStatfs);
  SetNamedMethod(env, binding, "readdir", FsReaddir);
  SetNamedMethod(env, binding, "realpath", FsRealpath);
  SetNamedMethod(env, binding, "read", FsRead);
  SetNamedMethod(env, binding, "readBuffers", FsReadBuffers);
  SetNamedMethod(env, binding, "writeBuffer", FsWriteBuffer);
  SetNamedMethod(env, binding, "writeString", FsWriteString);
  SetNamedMethod(env, binding, "writeBuffers", FsWriteBuffers);
  SetNamedMethod(env, binding, "readFileUtf8", FsReadFileUtf8);
  SetNamedMethod(env, binding, "openFileHandle", FsOpenFileHandle);
  SetNamedMethod(env, binding, "internalModuleStat", FsInternalModuleStat);
  SetNamedMethod(env, binding, "legacyMainResolve", FsLegacyMainResolve);
  SetNamedMethod(env, binding, "cpSyncCheckPaths", FsCpSyncCheckPaths);
  SetNamedMethod(env, binding, "cpSyncOverrideFile", FsCpSyncOverrideFile);
  SetNamedMethod(env, binding, "cpSyncCopyDir", FsCpSyncCopyDir);
  SetNamedMethod(env, binding, "mkdtemp", FsMkdtemp);
  SetNamedMethod(env, binding, "mkdir", FsMkdir);
  SetNamedMethod(env, binding, "rename", FsRename);
  SetNamedMethod(env, binding, "copyFile", FsCopyFile);
  SetNamedMethod(env, binding, "readlink", FsReadlink);
  SetNamedMethod(env, binding, "symlink", FsSymlink);
  SetNamedMethod(env, binding, "unlink", FsUnlink);
  SetNamedMethod(env, binding, "rmdir", FsRmdir);
  SetNamedMethod(env, binding, "ftruncate", FsFtruncate);
  SetNamedMethod(env, binding, "fsync", FsFsync);
  SetNamedMethod(env, binding, "chmod", FsChmod);
  SetNamedMethod(env, binding, "fchmod", FsFchmod);
  SetNamedMethod(env, binding, "utimes", FsUtimes);
  SetNamedMethod(env, binding, "futimes", FsFutimes);
  SetNamedMethod(env, binding, "lutimes", FsLutimes);
  SetNamedMethod(env, binding, "link", FsLink);
  SetNamedMethod(env, binding, "fdatasync", FsFdatasync);
  SetNamedMethod(env, binding, "chown", FsChown);
  SetNamedMethod(env, binding, "fchown", FsFchown);
  SetNamedMethod(env, binding, "lchown", FsLchown);

  const char* hidden_props[] = {
      "accessSync",        "readSync",         "writeSync",         "writeSyncString",
      "O_RDONLY",          "O_WRONLY",         "O_RDWR",            "O_CREAT",
      "O_TRUNC",           "O_APPEND",         "O_EXCL",            "O_SYNC",
      "O_NOATIME",         "UV_DIRENT_UNKNOWN","UV_DIRENT_FILE",    "UV_DIRENT_DIR",
      "UV_DIRENT_LINK",    "UV_DIRENT_FIFO",   "UV_DIRENT_SOCKET",  "UV_DIRENT_CHAR",
      "UV_DIRENT_BLOCK",   "F_OK",             "R_OK",              "W_OK",
      "X_OK",              "S_IFMT",           "S_IFREG",           "S_IFDIR",
      "S_IFBLK",           "S_IFCHR",          "S_IFLNK",           "S_IFIFO",
      "S_IFSOCK",          "S_IRWXU",          "S_IRUSR",           "S_IWUSR",
      "S_IXUSR",           "S_IRWXG",          "S_IRGRP",           "S_IWGRP",
      "S_IXGRP",           "S_IRWXO",          "S_IROTH",           "S_IWOTH",
      "S_IXOTH",           "COPYFILE_EXCL",    "COPYFILE_FICLONE",  "COPYFILE_FICLONE_FORCE",
      "UV_FS_COPYFILE_EXCL","UV_FS_COPYFILE_FICLONE","UV_FS_COPYFILE_FICLONE_FORCE",
      "UV_FS_SYMLINK_DIR", "UV_FS_SYMLINK_JUNCTION",
  };
  for (const char* name : hidden_props) DeleteNamedProperty(env, binding, name);

  return binding;
}

}  // namespace internal_binding
