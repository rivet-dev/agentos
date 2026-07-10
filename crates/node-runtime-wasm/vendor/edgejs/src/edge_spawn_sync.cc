#include "edge_spawn_sync.h"

#include <cerrno>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <new>
#include <string>
#include <string_view>
#include <vector>

#include <uv.h>

namespace {

enum class StdioMode {
  kPipe,
  kInheritFd,
  kIgnore,
};

struct StdioSpec {
  StdioMode mode = StdioMode::kIgnore;
  bool readable = false;
  bool writable = false;
  int inherit_fd = -1;
  std::vector<uint8_t> input;
};

struct SpawnOptions {
  std::string file;
  std::vector<std::string> args;
  std::string cwd;
  int64_t timeout_ms = 0;
  int64_t max_buffer = 1024 * 1024;
  int32_t kill_signal = SIGTERM;
  bool detached = false;
  bool windows_hide = false;
  bool windows_verbatim_arguments = false;
  bool has_uid = false;
  bool has_gid = false;
  int32_t uid = 0;
  int32_t gid = 0;
  std::vector<std::string> env_pairs;
  std::vector<StdioSpec> stdio;
};

struct SpawnSyncRunner;

struct OutputChunk {
  static constexpr unsigned int kBufferSize = 65536;

  char data[kBufferSize];
  unsigned int used = 0;
  std::unique_ptr<OutputChunk> next;
};

enum class PipeLifecycle {
  kUninitialized = 0,
  kInitialized,
  kStarted,
  kClosing,
  kClosed,
};

struct SyncPipe {
  SpawnSyncRunner* runner = nullptr;
  uint32_t child_fd = 0;
  uv_pipe_t pipe{};
  bool readable = false;
  bool writable = false;
  bool shutdown_pending = false;
  uv_write_t write_req{};
  uv_shutdown_t shutdown_req{};
  std::vector<uint8_t> input_storage;
  uv_buf_t input_buf{};
  std::unique_ptr<OutputChunk> first_output;
  OutputChunk* last_output = nullptr;
  PipeLifecycle lifecycle = PipeLifecycle::kUninitialized;
};

struct SpawnSyncRunner {
  uv_loop_t loop{};
  bool loop_initialized = false;

  uv_process_t process{};
  bool process_spawned = false;
  bool process_exited = false;
  int64_t exit_status = -1;
  int term_signal = 0;

  uv_timer_t timer{};
  bool timer_initialized = false;

  std::vector<SyncPipe> pipes;
  std::vector<uv_stdio_container_t> stdio;

  int kill_signal = SIGTERM;
  int64_t max_buffer = 1024 * 1024;
  int64_t buffered_output_size = 0;

  bool timed_out = false;
  bool kill_attempted = false;

  int error = 0;
  int pipe_error = 0;
};

bool GetNamedProperty(napi_env env, napi_value obj, const char* name, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  bool has = false;
  if (napi_has_named_property(env, obj, name, &has) != napi_ok || !has) return false;
  return napi_get_named_property(env, obj, name, out) == napi_ok && *out != nullptr;
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return "";
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

bool CoerceValueToUtf8(napi_env env, napi_value value, std::string* out) {
  if (out == nullptr || value == nullptr) return false;
  napi_value as_string = nullptr;
  if (napi_coerce_to_string(env, value, &as_string) != napi_ok || as_string == nullptr) return false;
  *out = ValueToUtf8(env, as_string);
  return true;
}

bool GetInt32Maybe(napi_env env, napi_value obj, const char* name, int32_t* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  if (!GetNamedProperty(env, obj, name, &value) || value == nullptr) return false;
  return napi_get_value_int32(env, value, out) == napi_ok;
}

bool GetBoolMaybe(napi_env env, napi_value obj, const char* name, bool* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  if (!GetNamedProperty(env, obj, name, &value) || value == nullptr) return false;
  return napi_get_value_bool(env, value, out) == napi_ok;
}

bool GetCoercedBoolMaybe(napi_env env, napi_value obj, const char* name, bool* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  if (!GetNamedProperty(env, obj, name, &value) || value == nullptr || IsNullOrUndefined(env, value)) return false;
  napi_value bool_value = nullptr;
  if (napi_coerce_to_bool(env, value, &bool_value) != napi_ok || bool_value == nullptr) return false;
  return napi_get_value_bool(env, bool_value, out) == napi_ok;
}

bool GetStrictInt32Maybe(napi_env env, napi_value obj, const char* name, int32_t* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  if (!GetNamedProperty(env, obj, name, &value) || value == nullptr || IsNullOrUndefined(env, value)) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) return false;

  double number = 0;
  if (napi_get_value_double(env, value, &number) != napi_ok) return false;
  if (!std::isfinite(number) || number < static_cast<double>(INT32_MIN) ||
      number > static_cast<double>(INT32_MAX) || std::trunc(number) != number) {
    return false;
  }

  return napi_get_value_int32(env, value, out) == napi_ok;
}

void SetDefaultStdioSpec(uint32_t child_fd, StdioSpec* spec) {
  if (spec == nullptr) return;
  spec->mode = child_fd < 3 ? StdioMode::kPipe : StdioMode::kIgnore;
  spec->readable = child_fd == 0;
  spec->writable = child_fd != 0;
  spec->inherit_fd = child_fd < 3 ? static_cast<int>(child_fd) : -1;
  spec->input.clear();
}

bool ParseStdioModeString(uint32_t child_fd, const std::string& mode, StdioSpec* out) {
  if (out == nullptr) return false;
  if (mode == "pipe" || mode == "overlapped") {
    out->mode = StdioMode::kPipe;
    out->readable = child_fd == 0;
    out->writable = child_fd != 0;
    out->inherit_fd = -1;
    return true;
  }
  if (mode == "inherit") {
    out->mode = StdioMode::kInheritFd;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = static_cast<int>(child_fd);
    return true;
  }
  if (mode == "ignore") {
    out->mode = StdioMode::kIgnore;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = -1;
    return true;
  }
  return false;
}

bool ReadInputBytes(napi_env env, napi_value input_val, std::vector<uint8_t>* out) {
  if (out == nullptr || input_val == nullptr) return false;
  out->clear();

  bool is_buffer = false;
  if (napi_is_buffer(env, input_val, &is_buffer) == napi_ok && is_buffer) {
    void* ptr = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, input_val, &ptr, &len) == napi_ok && ptr != nullptr && len > 0) {
      const uint8_t* bytes = static_cast<const uint8_t*>(ptr);
      out->assign(bytes, bytes + len);
    }
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, input_val, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type;
    size_t element_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env, input_val, &ta_type, &element_len, &raw, &ab, &byte_offset) != napi_ok ||
        raw == nullptr) {
      return false;
    }
    size_t bytes = element_len;
    switch (ta_type) {
      case napi_uint16_array:
      case napi_int16_array:
      case napi_float16_array:
        bytes *= 2;
        break;
      case napi_uint32_array:
      case napi_int32_array:
      case napi_float32_array:
        bytes *= 4;
        break;
      case napi_float64_array:
      case napi_bigint64_array:
      case napi_biguint64_array:
        bytes *= 8;
        break;
      default:
        break;
    }
    const uint8_t* data = static_cast<const uint8_t*>(raw);
    out->assign(data, data + bytes);
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, input_val, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_len = 0;
    void* raw = nullptr;
    napi_value ab = nullptr;
    size_t byte_offset = 0;
    if (napi_get_dataview_info(env, input_val, &byte_len, &raw, &ab, &byte_offset) != napi_ok ||
        ab == nullptr) {
      return false;
    }
    (void)raw;
    void* ab_raw = nullptr;
    size_t ab_len = 0;
    if (napi_get_arraybuffer_info(env, ab, &ab_raw, &ab_len) != napi_ok || ab_raw == nullptr) {
      return false;
    }
    if (byte_offset > ab_len || byte_len > (ab_len - byte_offset)) {
      return false;
    }
    const uint8_t* data = static_cast<const uint8_t*>(ab_raw) + byte_offset;
    out->assign(data, data + byte_len);
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, input_val, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t byte_len = 0;
    if (napi_get_arraybuffer_info(env, input_val, &raw, &byte_len) != napi_ok || raw == nullptr) return false;
    const uint8_t* data = static_cast<const uint8_t*>(raw);
    out->assign(data, data + byte_len);
    return true;
  }

  napi_valuetype value_type = napi_undefined;
  if (napi_typeof(env, input_val, &value_type) == napi_ok && value_type == napi_string) {
    std::string text = ValueToUtf8(env, input_val);
    out->assign(text.begin(), text.end());
    return true;
  }

  return false;
}

bool ExtractFdFromWrapObject(napi_env env, napi_value value, int32_t* fd) {
  if (fd == nullptr || value == nullptr) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_object) return false;

  int32_t direct_fd = -1;
  if (GetInt32Maybe(env, value, "fd", &direct_fd)) {
    *fd = direct_fd;
    return true;
  }

  napi_value nested = nullptr;
  if (GetNamedProperty(env, value, "handle", &nested) && nested != nullptr) {
    int32_t nested_fd = -1;
    if (GetInt32Maybe(env, nested, "fd", &nested_fd)) {
      *fd = nested_fd;
      return true;
    }
  }

  nested = nullptr;
  if (GetNamedProperty(env, value, "_handle", &nested) && nested != nullptr) {
    int32_t nested_fd = -1;
    if (GetInt32Maybe(env, nested, "fd", &nested_fd)) {
      *fd = nested_fd;
      return true;
    }
  }

  return false;
}

bool ParseStdioObject(napi_env env, uint32_t child_fd, napi_value stdio_desc, StdioSpec* out) {
  if (out == nullptr || stdio_desc == nullptr) return false;

  int32_t explicit_fd = -1;
  bool has_explicit_fd = GetInt32Maybe(env, stdio_desc, "fd", &explicit_fd);

  napi_value type_val = nullptr;
  std::string type_name;
  if (GetNamedProperty(env, stdio_desc, "type", &type_val) && type_val != nullptr) {
    if (!CoerceValueToUtf8(env, type_val, &type_name)) return false;
  }

  if (type_name.empty() && has_explicit_fd) {
    out->mode = StdioMode::kInheritFd;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = explicit_fd;
    return true;
  }

  if (type_name == "ignore") {
    out->mode = StdioMode::kIgnore;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = -1;
    return true;
  }

  if (type_name == "inherit" || type_name == "fd") {
    out->mode = StdioMode::kInheritFd;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = has_explicit_fd ? explicit_fd : static_cast<int>(child_fd);
    return true;
  }

  if (type_name == "wrap") {
    int32_t wrap_fd = -1;
    if (!ExtractFdFromWrapObject(env, stdio_desc, &wrap_fd)) return false;
    out->mode = StdioMode::kInheritFd;
    out->readable = false;
    out->writable = false;
    out->inherit_fd = wrap_fd;
    return true;
  }

  if (type_name == "pipe" || type_name == "overlapped") {
    out->mode = StdioMode::kPipe;
    out->readable = child_fd == 0;
    out->writable = child_fd != 0;
    out->inherit_fd = -1;

    bool readable = false;
    if (GetBoolMaybe(env, stdio_desc, "readable", &readable)) {
      out->readable = readable;
    }

    bool writable = false;
    if (GetBoolMaybe(env, stdio_desc, "writable", &writable)) {
      out->writable = writable;
    }

    napi_value input_val = nullptr;
    if (GetNamedProperty(env, stdio_desc, "input", &input_val) && !IsNullOrUndefined(env, input_val)) {
      if (!ReadInputBytes(env, input_val, &out->input)) return false;
    }
    return true;
  }

  return false;
}

bool ParseSpawnOptions(napi_env env, napi_value value, SpawnOptions* out) {
  if (out == nullptr) return false;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_object) return false;

  napi_value file_val = nullptr;
  if (GetNamedProperty(env, value, "file", &file_val)) {
    if (!CoerceValueToUtf8(env, file_val, &out->file)) return false;
  }
  if (out->file.empty()) return false;

  napi_value args_val = nullptr;
  bool has_args = GetNamedProperty(env, value, "args", &args_val);
  bool is_array = false;
  if (has_args) {
    if (napi_is_array(env, args_val, &is_array) != napi_ok || !is_array) return false;
    uint32_t len = 0;
    if (napi_get_array_length(env, args_val, &len) == napi_ok) {
      out->args.reserve(static_cast<size_t>(len));
      for (uint32_t i = 0; i < len; ++i) {
        napi_value elem = nullptr;
        if (napi_get_element(env, args_val, i, &elem) != napi_ok || elem == nullptr) continue;
        std::string arg;
        if (!CoerceValueToUtf8(env, elem, &arg)) return false;
        out->args.push_back(arg);
      }
    }
  }
  if (out->args.empty()) {
    out->args.push_back(out->file);
  }

  napi_value cwd_val = nullptr;
  if (GetNamedProperty(env, value, "cwd", &cwd_val)) {
    if (!IsNullOrUndefined(env, cwd_val) && !CoerceValueToUtf8(env, cwd_val, &out->cwd)) return false;
  }

  napi_value timeout_val = nullptr;
  if (GetNamedProperty(env, value, "timeout", &timeout_val)) {
    napi_valuetype timeout_t = napi_undefined;
    if (napi_typeof(env, timeout_val, &timeout_t) != napi_ok) return false;
    if (timeout_t != napi_undefined && timeout_t != napi_null) {
      if (timeout_t != napi_number) return false;
      int64_t timeout = 0;
      if (napi_get_value_int64(env, timeout_val, &timeout) == napi_ok && timeout > 0) {
        out->timeout_ms = timeout;
      }
    }
  }

  napi_value max_buffer_val = nullptr;
  if (GetNamedProperty(env, value, "maxBuffer", &max_buffer_val)) {
    napi_valuetype mb_t = napi_undefined;
    if (napi_typeof(env, max_buffer_val, &mb_t) != napi_ok) return false;
    if (mb_t != napi_undefined && mb_t != napi_null) {
      if (mb_t != napi_number) return false;
      double mb = 0;
      if (napi_get_value_double(env, max_buffer_val, &mb) == napi_ok && mb >= 0) {
        if (mb > static_cast<double>(INT64_MAX)) out->max_buffer = INT64_MAX;
        else out->max_buffer = static_cast<int64_t>(mb);
      }
    }
  }

  napi_value env_pairs_val = nullptr;
  bool has_env_pairs = GetNamedProperty(env, value, "envPairs", &env_pairs_val);
  bool env_is_array = false;
  if (has_env_pairs && !IsNullOrUndefined(env, env_pairs_val)) {
    if (napi_is_array(env, env_pairs_val, &env_is_array) != napi_ok || !env_is_array) return false;
    uint32_t len = 0;
    if (napi_get_array_length(env, env_pairs_val, &len) == napi_ok) {
      out->env_pairs.reserve(static_cast<size_t>(len));
      for (uint32_t i = 0; i < len; ++i) {
        napi_value elem = nullptr;
        if (napi_get_element(env, env_pairs_val, i, &elem) != napi_ok || elem == nullptr) continue;
        std::string pair;
        if (!CoerceValueToUtf8(env, elem, &pair)) return false;
        out->env_pairs.push_back(pair);
      }
    }
  }

  napi_value kill_signal_val = nullptr;
  if (GetNamedProperty(env, value, "killSignal", &kill_signal_val)) {
    napi_valuetype ks_t = napi_undefined;
    if (napi_typeof(env, kill_signal_val, &ks_t) != napi_ok) return false;
    if (ks_t != napi_undefined && ks_t != napi_null) {
      if (ks_t != napi_number) return false;
      int32_t ks = SIGTERM;
      if (napi_get_value_int32(env, kill_signal_val, &ks) == napi_ok) {
        out->kill_signal = ks;
      }
    }
  }

  (void)GetCoercedBoolMaybe(env, value, "detached", &out->detached);
  (void)GetCoercedBoolMaybe(env, value, "windowsHide", &out->windows_hide);
  (void)GetCoercedBoolMaybe(env, value, "windowsVerbatimArguments", &out->windows_verbatim_arguments);

  out->has_uid = GetStrictInt32Maybe(env, value, "uid", &out->uid);
  if (!out->has_uid) {
    napi_value uid_val = nullptr;
    if (GetNamedProperty(env, value, "uid", &uid_val) && !IsNullOrUndefined(env, uid_val)) return false;
  }

  out->has_gid = GetStrictInt32Maybe(env, value, "gid", &out->gid);
  if (!out->has_gid) {
    napi_value gid_val = nullptr;
    if (GetNamedProperty(env, value, "gid", &gid_val) && !IsNullOrUndefined(env, gid_val)) return false;
  }

  out->stdio.resize(3);
  for (uint32_t i = 0; i < 3; ++i) {
    SetDefaultStdioSpec(i, &out->stdio[i]);
  }

  napi_value stdio_val = nullptr;
  bool has_stdio = GetNamedProperty(env, value, "stdio", &stdio_val);
  napi_valuetype stdio_type = napi_undefined;
  if (has_stdio && napi_typeof(env, stdio_val, &stdio_type) == napi_ok && stdio_type == napi_string) {
    std::string mode = ValueToUtf8(env, stdio_val);
    for (uint32_t i = 0; i < 3; ++i) {
      if (!ParseStdioModeString(i, mode, &out->stdio[i])) return false;
    }
  }

  bool stdio_is_array = false;
  if (has_stdio && napi_is_array(env, stdio_val, &stdio_is_array) == napi_ok && stdio_is_array) {
    uint32_t stdio_len = 0;
    if (napi_get_array_length(env, stdio_val, &stdio_len) == napi_ok) {
      const uint32_t target_len = stdio_len < 3 ? 3 : stdio_len;
      out->stdio.resize(target_len);
      for (uint32_t i = 0; i < target_len; ++i) {
        SetDefaultStdioSpec(i, &out->stdio[i]);
      }
      for (uint32_t i = 0; i < stdio_len; ++i) {
        napi_value stdio_desc = nullptr;
        if (napi_get_element(env, stdio_val, i, &stdio_desc) != napi_ok || stdio_desc == nullptr ||
            IsNullOrUndefined(env, stdio_desc)) {
          continue;
        }

        napi_valuetype entry_type = napi_undefined;
        if (napi_typeof(env, stdio_desc, &entry_type) != napi_ok) return false;

        if (entry_type == napi_string) {
          std::string mode = ValueToUtf8(env, stdio_desc);
          if (!ParseStdioModeString(i, mode, &out->stdio[i])) return false;
          continue;
        }

        if (entry_type == napi_number) {
          int32_t fd = -1;
          if (napi_get_value_int32(env, stdio_desc, &fd) != napi_ok) return false;
          if (fd < 0) {
            out->stdio[i].mode = StdioMode::kPipe;
            out->stdio[i].readable = i == 0;
            out->stdio[i].writable = i != 0;
            out->stdio[i].inherit_fd = -1;
          } else {
            out->stdio[i].mode = StdioMode::kInheritFd;
            out->stdio[i].readable = false;
            out->stdio[i].writable = false;
            out->stdio[i].inherit_fd = fd;
          }
          continue;
        }

        if (entry_type != napi_object) return false;
        if (!ParseStdioObject(env, i, stdio_desc, &out->stdio[i])) return false;
      }
    }
  } else if (has_stdio) {
    return false;
  }

  napi_value input_val = nullptr;
  if (GetNamedProperty(env, value, "input", &input_val) && !IsNullOrUndefined(env, input_val)) {
    if (!ReadInputBytes(env, input_val, &out->stdio[0].input)) return false;
  }

  return true;
}

const char* SignalName(int signum) {
  switch (signum) {
    case SIGTERM: return "SIGTERM";
    case SIGKILL: return "SIGKILL";
    case SIGINT: return "SIGINT";
    case SIGABRT: return "SIGABRT";
    case SIGQUIT: return "SIGQUIT";
    case SIGSEGV: return "SIGSEGV";
    case SIGILL: return "SIGILL";
#if defined(SIGTRAP)
    case SIGTRAP: return "SIGTRAP";
#endif
#if defined(SIGBUS)
    case SIGBUS: return "SIGBUS";
#endif
    default: return nullptr;
  }
}

void UvCloseNoop(uv_handle_t* /*handle*/) {}

void SetErrorIfUnset(SpawnSyncRunner* runner, int error) {
  if (runner == nullptr || error == 0) return;
  if (runner->error == 0) runner->error = error;
}

void SetPipeErrorIfUnset(SpawnSyncRunner* runner, int error) {
  if (runner == nullptr || error == 0) return;
  if (runner->pipe_error == 0) runner->pipe_error = error;
}

OutputChunk* EnsureOutputChunk(SyncPipe* pipe) {
  if (pipe == nullptr) return nullptr;
  if (pipe->last_output != nullptr && pipe->last_output->used < OutputChunk::kBufferSize) {
    return pipe->last_output;
  }

  std::unique_ptr<OutputChunk> chunk(new (std::nothrow) OutputChunk());
  if (!chunk) return nullptr;

  OutputChunk* raw = chunk.get();
  if (!pipe->first_output) {
    pipe->first_output = std::move(chunk);
  } else {
    pipe->last_output->next = std::move(chunk);
  }
  pipe->last_output = raw;
  return raw;
}

size_t GetPipeOutputLength(const SyncPipe& pipe) {
  size_t length = 0;
  for (const OutputChunk* chunk = pipe.first_output.get(); chunk != nullptr; chunk = chunk->next.get()) {
    length += chunk->used;
  }
  return length;
}

void CopyPipeOutput(const SyncPipe& pipe, uint8_t* dest) {
  if (dest == nullptr) return;
  size_t offset = 0;
  for (const OutputChunk* chunk = pipe.first_output.get(); chunk != nullptr; chunk = chunk->next.get()) {
    if (chunk->used == 0) continue;
    std::memcpy(dest + offset, chunk->data, chunk->used);
    offset += chunk->used;
  }
}

void CloseHandleIfNeeded(uv_handle_t* handle) {
  if (handle == nullptr) return;
  if (!uv_is_closing(handle)) uv_close(handle, UvCloseNoop);
}

void WalkAndCloseHandle(uv_handle_t* handle, void* /*arg*/) {
  if (handle == nullptr) return;
  if (!uv_is_closing(handle)) uv_close(handle, UvCloseNoop);
}

void OnPipeClosed(uv_handle_t* handle) {
  if (handle == nullptr) return;
  auto* pipe = static_cast<SyncPipe*>(handle->data);
  if (pipe == nullptr) return;
  pipe->lifecycle = PipeLifecycle::kClosed;
}

void ClosePipeIfNeeded(SyncPipe* pipe) {
  if (pipe == nullptr) return;
  if (pipe->lifecycle != PipeLifecycle::kInitialized && pipe->lifecycle != PipeLifecycle::kStarted) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&pipe->pipe);
  if (!uv_is_closing(handle)) {
    uv_close(handle, OnPipeClosed);
    pipe->lifecycle = PipeLifecycle::kClosing;
  }
}

void CloseAllPipes(SpawnSyncRunner* runner) {
  if (runner == nullptr) return;
  for (auto& pipe : runner->pipes) {
    ClosePipeIfNeeded(&pipe);
  }
}

void CloseTimerIfNeeded(SpawnSyncRunner* runner) {
  if (runner == nullptr || !runner->timer_initialized) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&runner->timer);
  uv_ref(handle);
  if (!uv_is_closing(handle)) uv_close(handle, UvCloseNoop);
  runner->timer_initialized = false;
}

void KillAndClose(SpawnSyncRunner* runner) {
  if (runner == nullptr) return;
  if (!runner->kill_attempted) {
    runner->kill_attempted = true;
    if (runner->process_spawned && !runner->process_exited) {
      int rc = uv_process_kill(&runner->process, runner->kill_signal);
      if (rc < 0 && rc != UV_ESRCH) {
        SetErrorIfUnset(runner, rc);
        (void)uv_process_kill(&runner->process, SIGKILL);
      }
    }
  }
  CloseAllPipes(runner);
  CloseTimerIfNeeded(runner);
}

void StartPipeShutdown(SyncPipe* pipe);

void PipeShutdownCallback(uv_shutdown_t* req, int status) {
  if (req == nullptr) return;
  auto* pipe = static_cast<SyncPipe*>(req->data);
  if (pipe == nullptr || pipe->runner == nullptr) return;
  pipe->shutdown_pending = false;
  if (status < 0 && status != UV_ENOTCONN) {
    SetPipeErrorIfUnset(pipe->runner, status);
  }
  if (!pipe->writable) {
    ClosePipeIfNeeded(pipe);
  }
}

void StartPipeShutdown(SyncPipe* pipe) {
  if (pipe == nullptr) return;
  if (pipe->lifecycle != PipeLifecycle::kInitialized && pipe->lifecycle != PipeLifecycle::kStarted) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&pipe->pipe);
  if (uv_is_closing(handle) || pipe->shutdown_pending) return;

  pipe->shutdown_req.data = pipe;
  pipe->shutdown_pending = true;
  int rc = uv_shutdown(&pipe->shutdown_req,
                       reinterpret_cast<uv_stream_t*>(&pipe->pipe),
                       PipeShutdownCallback);
  if (rc == UV_ENOTCONN) {
    pipe->shutdown_pending = false;
    if (!pipe->writable) ClosePipeIfNeeded(pipe);
    return;
  }
  if (rc < 0) {
    pipe->shutdown_pending = false;
    SetPipeErrorIfUnset(pipe->runner, rc);
    if (!pipe->writable) ClosePipeIfNeeded(pipe);
  }
}

void PipeWriteCallback(uv_write_t* req, int status) {
  if (req == nullptr) return;
  auto* pipe = static_cast<SyncPipe*>(req->data);
  if (pipe == nullptr || pipe->runner == nullptr) return;
  if (status < 0) SetPipeErrorIfUnset(pipe->runner, status);
  StartPipeShutdown(pipe);
}

void AllocReadBuffer(uv_handle_t* handle, size_t /*suggested_size*/, uv_buf_t* buf) {
  if (buf == nullptr) return;
  auto* pipe = handle == nullptr ? nullptr : static_cast<SyncPipe*>(handle->data);
  if (pipe == nullptr || pipe->runner == nullptr || pipe->lifecycle != PipeLifecycle::kStarted) {
    *buf = uv_buf_init(nullptr, 0);
    return;
  }

  OutputChunk* chunk = EnsureOutputChunk(pipe);
  if (chunk == nullptr) {
    SetErrorIfUnset(pipe->runner, UV_ENOMEM);
    KillAndClose(pipe->runner);
    *buf = uv_buf_init(nullptr, 0);
    return;
  }

  *buf = uv_buf_init(chunk->data + chunk->used, OutputChunk::kBufferSize - chunk->used);
}

void OnProcessExit(uv_process_t* process, int64_t exit_status, int term_signal) {
  if (process == nullptr) return;
  auto* runner = static_cast<SpawnSyncRunner*>(process->data);
  if (runner == nullptr) return;

  runner->process_exited = true;
  runner->exit_status = exit_status;
  runner->term_signal = term_signal;

  CloseHandleIfNeeded(reinterpret_cast<uv_handle_t*>(process));
  CloseTimerIfNeeded(runner);
}

void OnKillTimer(uv_timer_t* timer) {
  if (timer == nullptr) return;
  auto* runner = static_cast<SpawnSyncRunner*>(timer->data);
  if (runner == nullptr) return;

  runner->timed_out = true;
  SetErrorIfUnset(runner, UV_ETIMEDOUT);
  KillAndClose(runner);
}

void OnPipeRead(uv_stream_t* stream, ssize_t nread, const uv_buf_t* buf) {
  auto* pipe = stream == nullptr ? nullptr : static_cast<SyncPipe*>(stream->data);
  auto* runner = pipe != nullptr ? pipe->runner : nullptr;

  if (nread > 0 && pipe != nullptr && runner != nullptr && pipe->last_output != nullptr &&
      buf != nullptr && buf->base != nullptr) {
    pipe->last_output->used += static_cast<unsigned int>(nread);
    runner->buffered_output_size += nread;
    if (runner->max_buffer > 0 && runner->buffered_output_size > runner->max_buffer) {
      SetErrorIfUnset(runner, UV_ENOBUFS);
      KillAndClose(runner);
    }
  } else if (nread == UV_EOF) {
    ClosePipeIfNeeded(pipe);
  } else if (nread < 0) {
    if (runner != nullptr) SetPipeErrorIfUnset(runner, static_cast<int>(nread));
    ClosePipeIfNeeded(pipe);
  }
}

int GetFinalError(const SpawnSyncRunner& runner) {
  if (runner.error != 0) return runner.error;
  return runner.pipe_error;
}

void CleanupRunner(SpawnSyncRunner* runner) {
  if (runner == nullptr || !runner->loop_initialized) return;

  if (runner->process_spawned) {
    CloseHandleIfNeeded(reinterpret_cast<uv_handle_t*>(&runner->process));
  }
  CloseAllPipes(runner);
  CloseTimerIfNeeded(runner);

  int close_rc = uv_loop_close(&runner->loop);
  while (close_rc == UV_EBUSY) {
    uv_walk(&runner->loop, WalkAndCloseHandle, nullptr);
    (void)uv_run(&runner->loop, UV_RUN_DEFAULT);
    close_rc = uv_loop_close(&runner->loop);
  }
  runner->loop_initialized = false;
}

napi_value MakeNull(napi_env env) {
  napi_value value = nullptr;
  napi_get_null(env, &value);
  return value;
}

napi_value MakeErrorResult(napi_env env, int error) {
  napi_value result = nullptr;
  napi_create_object(env, &result);
  napi_value error_value = nullptr;
  napi_create_int32(env, error, &error_value);
  napi_set_named_property(env, result, "error", error_value);
  napi_value status = nullptr;
  napi_get_null(env, &status);
  napi_set_named_property(env, result, "status", status);
  napi_set_named_property(env, result, "signal", status);
  napi_set_named_property(env, result, "output", status);
  napi_value pid = nullptr;
  napi_create_int32(env, 0, &pid);
  napi_set_named_property(env, result, "pid", pid);
  return result;
}

napi_value BuildOutputArray(napi_env env, const SpawnSyncRunner& runner) {
  napi_value output = nullptr;
  napi_create_array_with_length(env, runner.pipes.size(), &output);
  napi_value null_value = MakeNull(env);

  for (uint32_t i = 0; i < runner.pipes.size(); ++i) {
    const SyncPipe& pipe = runner.pipes[i];
    if (pipe.lifecycle == PipeLifecycle::kUninitialized || !pipe.writable) {
      napi_set_element(env, output, i, null_value);
      continue;
    }

    napi_value buffer = nullptr;
    const size_t output_length = GetPipeOutputLength(pipe);
    void* buffer_data = nullptr;
    napi_create_buffer(env, output_length, &buffer_data, &buffer);
    if (output_length > 0 && buffer_data != nullptr) {
      CopyPipeOutput(pipe, static_cast<uint8_t*>(buffer_data));
    }
    napi_set_element(env, output, i, buffer);
  }

  return output;
}

napi_value SpawnSync(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  SpawnOptions options;
  if (argc < 1 || argv[0] == nullptr || !ParseSpawnOptions(env, argv[0], &options)) {
    return MakeErrorResult(env, -EINVAL);
  }

  SpawnSyncRunner runner;
  runner.kill_signal = options.kill_signal;
  runner.max_buffer = options.max_buffer;
  runner.pipes.resize(options.stdio.size());
  runner.stdio.resize(options.stdio.size());

  int rc = uv_loop_init(&runner.loop);
  if (rc < 0) {
    return MakeErrorResult(env, rc);
  }
  runner.loop_initialized = true;

  for (uint32_t i = 0; i < options.stdio.size(); ++i) {
    const StdioSpec& spec = options.stdio[i];
    auto& pipe = runner.pipes[i];
    pipe.runner = &runner;
    pipe.child_fd = i;

    switch (spec.mode) {
      case StdioMode::kIgnore:
        runner.stdio[i].flags = UV_IGNORE;
        break;
      case StdioMode::kInheritFd:
        runner.stdio[i].flags = UV_INHERIT_FD;
        runner.stdio[i].data.fd = spec.inherit_fd;
        break;
      case StdioMode::kPipe: {
        if (!spec.readable && !spec.writable) {
          SetErrorIfUnset(&runner, UV_EINVAL);
          break;
        }
        rc = uv_pipe_init(&runner.loop, &pipe.pipe, 0);
        if (rc < 0) {
          SetErrorIfUnset(&runner, rc);
          break;
        }
        pipe.readable = spec.readable;
        pipe.writable = spec.writable;
        pipe.pipe.data = &pipe;
        pipe.input_storage = spec.input;
        pipe.lifecycle = PipeLifecycle::kInitialized;
        runner.stdio[i].flags = static_cast<uv_stdio_flags>(UV_CREATE_PIPE |
                                                             (pipe.readable ? UV_READABLE_PIPE : 0) |
                                                             (pipe.writable ? UV_WRITABLE_PIPE : 0));
        runner.stdio[i].data.stream = reinterpret_cast<uv_stream_t*>(&pipe.pipe);
        break;
      }
    }
  }

  std::vector<char*> exec_args;
  exec_args.reserve(options.args.size() + 1);
  for (std::string& arg : options.args) {
    exec_args.push_back(const_cast<char*>(arg.c_str()));
  }
  exec_args.push_back(nullptr);

  std::vector<char*> exec_env;
  if (!options.env_pairs.empty()) {
    exec_env.reserve(options.env_pairs.size() + 1);
    for (std::string& kv : options.env_pairs) {
      exec_env.push_back(const_cast<char*>(kv.c_str()));
    }
    exec_env.push_back(nullptr);
  }

  if (GetFinalError(runner) == 0) {
    uv_process_options_t uv_options{};
    uv_options.file = options.file.c_str();
    uv_options.args = exec_args.data();
    uv_options.exit_cb = OnProcessExit;
    uv_options.flags = 0;
    if (!options.cwd.empty()) uv_options.cwd = options.cwd.c_str();
    if (!exec_env.empty()) uv_options.env = exec_env.data();
    if (options.detached) uv_options.flags |= UV_PROCESS_DETACHED;
    if (options.windows_hide) uv_options.flags |= UV_PROCESS_WINDOWS_HIDE;
    if (options.windows_verbatim_arguments) uv_options.flags |= UV_PROCESS_WINDOWS_VERBATIM_ARGUMENTS;
    if (options.has_uid) {
      uv_options.flags |= UV_PROCESS_SETUID;
      uv_options.uid = static_cast<uv_uid_t>(options.uid);
    }
    if (options.has_gid) {
      uv_options.flags |= UV_PROCESS_SETGID;
      uv_options.gid = static_cast<uv_gid_t>(options.gid);
    }
    uv_options.stdio_count = static_cast<int>(runner.stdio.size());
    uv_options.stdio = runner.stdio.data();

    rc = uv_spawn(&runner.loop, &runner.process, &uv_options);
    if (rc < 0) {
      SetErrorIfUnset(&runner, rc);
    } else {
      runner.process_spawned = true;
      runner.process.data = &runner;
    }
  }

  if (runner.process_spawned) {
    for (auto& pipe : runner.pipes) {
      if (pipe.lifecycle != PipeLifecycle::kInitialized) continue;
      pipe.lifecycle = PipeLifecycle::kStarted;

      if (pipe.writable) {
        rc = uv_read_start(reinterpret_cast<uv_stream_t*>(&pipe.pipe), AllocReadBuffer, OnPipeRead);
        if (rc < 0) {
          SetPipeErrorIfUnset(&runner, rc);
          KillAndClose(&runner);
          break;
        }
      }

      if (pipe.readable) {
        if (!pipe.input_storage.empty()) {
          pipe.input_buf = uv_buf_init(reinterpret_cast<char*>(pipe.input_storage.data()),
                                       static_cast<unsigned int>(pipe.input_storage.size()));
          pipe.write_req.data = &pipe;
          rc = uv_write(&pipe.write_req,
                        reinterpret_cast<uv_stream_t*>(&pipe.pipe),
                        &pipe.input_buf,
                        1,
                        PipeWriteCallback);
          if (rc < 0) {
            SetPipeErrorIfUnset(&runner, rc);
            StartPipeShutdown(&pipe);
          }
        } else {
          StartPipeShutdown(&pipe);
        }
      }
    }

    if (options.timeout_ms > 0) {
      rc = uv_timer_init(&runner.loop, &runner.timer);
      if (rc < 0) {
        SetErrorIfUnset(&runner, rc);
      } else {
        runner.timer_initialized = true;
        runner.timer.data = &runner;
        uv_unref(reinterpret_cast<uv_handle_t*>(&runner.timer));
        rc = uv_timer_start(&runner.timer, OnKillTimer, static_cast<uint64_t>(options.timeout_ms), 0);
        if (rc < 0) {
          SetErrorIfUnset(&runner, rc);
          CloseTimerIfNeeded(&runner);
        }
      }
    }
  }

  if (GetFinalError(runner) != 0 && runner.process_spawned) {
    KillAndClose(&runner);
  }

  (void)uv_run(&runner.loop, UV_RUN_DEFAULT);

  napi_value result = nullptr;
  napi_create_object(env, &result);

  const int final_error = GetFinalError(runner);
  if (final_error != 0) {
    napi_value error_value = nullptr;
    napi_create_int32(env, final_error, &error_value);
    napi_set_named_property(env, result, "error", error_value);
  }

  napi_value null_value = MakeNull(env);
  if (!runner.process_spawned || !runner.process_exited) {
    napi_set_named_property(env, result, "status", null_value);
    napi_set_named_property(env, result, "signal", null_value);
  } else if (runner.term_signal > 0) {
    napi_set_named_property(env, result, "status", null_value);
    const char* sig_name = SignalName(runner.term_signal);
    if (sig_name != nullptr) {
      napi_value signal_value = nullptr;
      napi_create_string_utf8(env, sig_name, NAPI_AUTO_LENGTH, &signal_value);
      napi_set_named_property(env, result, "signal", signal_value);
    } else {
      napi_set_named_property(env, result, "signal", null_value);
    }
  } else {
    napi_value status_value = nullptr;
    napi_create_int32(env, static_cast<int32_t>(runner.exit_status), &status_value);
    napi_set_named_property(env, result, "status", status_value);
    napi_set_named_property(env, result, "signal", null_value);
  }

  if (runner.process_spawned) {
    napi_value output = BuildOutputArray(env, runner);
    napi_set_named_property(env, result, "output", output);
  } else {
    napi_set_named_property(env, result, "output", null_value);
  }

  napi_value pid_value = nullptr;
  int32_t pid = runner.process_spawned ? static_cast<int32_t>(runner.process.pid) : 0;
  napi_create_int32(env, pid, &pid_value);
  napi_set_named_property(env, result, "pid", pid_value);

  CleanupRunner(&runner);
  return result;
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) != napi_ok || fn == nullptr) {
    return;
  }
  napi_set_named_property(env, obj, name, fn);
}

}  // namespace

napi_value EdgeInstallSpawnSyncBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;
  SetMethod(env, binding, "spawn", SpawnSync);
  return binding;
}
