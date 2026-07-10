#include "internal_binding/binding_initializers.h"

#include <cstdint>
#include <cstring>
#include <string>

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "llhttp.h"

namespace internal_binding {

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

bool SetNamedFunction(napi_env env, napi_value object, const char* name, napi_callback callback) {
  napi_value fn = nullptr;
  return napi_create_function(env, name, NAPI_AUTO_LENGTH, callback, nullptr, &fn) == napi_ok &&
         fn != nullptr &&
         napi_set_named_property(env, object, name, fn) == napi_ok;
}

struct UndiciBindingState {
  explicit UndiciBindingState(napi_env env_in) : env(env_in) {}
  ~UndiciBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

struct NativeLlhttpParser {
  explicit NativeLlhttpParser(napi_env env_in) : env(env_in) {}
  ~NativeLlhttpParser() {
    DeleteRefIfPresent(env, &parser_ref);
  }

  napi_env env = nullptr;
  napi_ref parser_ref = nullptr;
  llhttp_t parser{};
  llhttp_settings_t settings{};
  const char* current_data = nullptr;
  size_t current_length = 0;
  size_t last_error_offset = 0;
  std::string last_error_reason;
  bool pending_exception = false;
};

struct NativeLlhttpHandle {
  NativeLlhttpParser* parser = nullptr;
};

UndiciBindingState* GetUndiciBindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<UndiciBindingState>(env, kEdgeEnvironmentSlotUndiciBindingState);
}

UndiciBindingState& EnsureUndiciBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<UndiciBindingState>(env, kEdgeEnvironmentSlotUndiciBindingState);
}

void NativeLlhttpHandleFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  auto* handle = static_cast<NativeLlhttpHandle*>(data);
  if (handle == nullptr) return;
  delete handle->parser;
  handle->parser = nullptr;
  delete handle;
}

NativeLlhttpParser* GetNativeParser(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  void* raw = nullptr;
  if (napi_get_value_external(env, value, &raw) != napi_ok || raw == nullptr) {
    napi_throw_type_error(env, nullptr, "Invalid native llhttp parser handle");
    return nullptr;
  }
  auto* handle = static_cast<NativeLlhttpHandle*>(raw);
  if (handle->parser == nullptr) {
    napi_throw_type_error(env, nullptr, "Native llhttp parser has been freed");
    return nullptr;
  }
  return handle->parser;
}

bool ReadBufferLike(napi_env env, napi_value value, const char** data, size_t* length) {
  if (env == nullptr || value == nullptr || data == nullptr || length == nullptr) return false;
  void* raw = nullptr;
  size_t len = 0;
  if (napi_get_buffer_info(env, value, &raw, &len) == napi_ok) {
    *data = static_cast<const char*>(raw);
    *length = len;
    return true;
  }

  napi_typedarray_type type = napi_uint8_array;
  size_t element_length = 0;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env, value, &type, &element_length, &raw, &arraybuffer, &byte_offset) != napi_ok ||
      (type != napi_uint8_array && type != napi_uint8_clamped_array)) {
    return false;
  }
  *data = static_cast<const char*>(raw);
  *length = element_length;
  return true;
}

napi_value MakeBufferCopy(napi_env env, const char* data, size_t length) {
  napi_value out = nullptr;
  const char* source = length == 0 ? nullptr : data;
  if (napi_create_buffer_copy(env, length, source, nullptr, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

int CallbackResultToInt32(napi_env env, napi_value result) {
  if (result == nullptr || IsUndefined(env, result)) return 0;
  int32_t out = 0;
  if (napi_get_value_int32(env, result, &out) != napi_ok) return 0;
  return out;
}

int CallParserMethod(NativeLlhttpParser* parser, const char* method_name, size_t argc, napi_value* argv) {
  if (parser == nullptr || parser->env == nullptr || method_name == nullptr) return -1;
  napi_env env = parser->env;
  napi_handle_scope scope = nullptr;
  const bool has_scope = napi_open_handle_scope(env, &scope) == napi_ok && scope != nullptr;

  napi_value parser_object = GetRefValue(env, parser->parser_ref);
  napi_value method = nullptr;
  napi_value result = nullptr;
  napi_status status = napi_invalid_arg;
  if (parser_object != nullptr &&
      napi_get_named_property(env, parser_object, method_name, &method) == napi_ok &&
      method != nullptr) {
    napi_valuetype method_type = napi_undefined;
    if (napi_typeof(env, method, &method_type) == napi_ok && method_type == napi_function) {
      status = napi_call_function(env, parser_object, method, argc, argv, &result);
    }
  }

  int out = 0;
  if (status == napi_ok) {
    out = CallbackResultToInt32(env, result);
  } else {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      parser->pending_exception = true;
    }
    out = -1;
  }

  if (has_scope) {
    napi_close_handle_scope(env, scope);
  }
  return out;
}

int CallParserMethod0(NativeLlhttpParser* parser, const char* method_name) {
  return CallParserMethod(parser, method_name, 0, nullptr);
}

int CallParserMethodBuffer(NativeLlhttpParser* parser,
                           const char* method_name,
                           const char* data,
                           size_t length) {
  if (parser == nullptr || parser->env == nullptr) return -1;
  napi_value buffer = MakeBufferCopy(parser->env, data, length);
  if (buffer == nullptr) return -1;
  return CallParserMethod(parser, method_name, 1, &buffer);
}

NativeLlhttpParser* ParserFromLlhttp(llhttp_t* llp) {
  return llp == nullptr ? nullptr : static_cast<NativeLlhttpParser*>(llp->data);
}

int OnMessageBegin(llhttp_t* llp) {
  return CallParserMethod0(ParserFromLlhttp(llp), "onMessageBegin");
}

int OnStatus(llhttp_t* llp, const char* at, size_t length) {
  return CallParserMethodBuffer(ParserFromLlhttp(llp), "onStatus", at, length);
}

int OnHeaderField(llhttp_t* llp, const char* at, size_t length) {
  return CallParserMethodBuffer(ParserFromLlhttp(llp), "onHeaderField", at, length);
}

int OnHeaderValue(llhttp_t* llp, const char* at, size_t length) {
  return CallParserMethodBuffer(ParserFromLlhttp(llp), "onHeaderValue", at, length);
}

int OnHeadersComplete(llhttp_t* llp) {
  NativeLlhttpParser* parser = ParserFromLlhttp(llp);
  if (parser == nullptr || parser->env == nullptr) return -1;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_create_uint32(parser->env, llp->status_code, &argv[0]) != napi_ok ||
      napi_get_boolean(parser->env, llp->upgrade != 0, &argv[1]) != napi_ok ||
      napi_get_boolean(parser->env, llhttp_should_keep_alive(llp) != 0, &argv[2]) != napi_ok) {
    return -1;
  }
  return CallParserMethod(parser, "onHeadersComplete", 3, argv);
}

int OnBody(llhttp_t* llp, const char* at, size_t length) {
  return CallParserMethodBuffer(ParserFromLlhttp(llp), "onBody", at, length);
}

int OnMessageComplete(llhttp_t* llp) {
  return CallParserMethod0(ParserFromLlhttp(llp), "onMessageComplete");
}

void CaptureLlhttpError(NativeLlhttpParser* parser, llhttp_errno_t err) {
  if (parser == nullptr) return;
  const char* pos = llhttp_get_error_pos(&parser->parser);
  if (pos != nullptr && parser->current_data != nullptr &&
      pos >= parser->current_data &&
      pos <= parser->current_data + parser->current_length) {
    parser->last_error_offset = static_cast<size_t>(pos - parser->current_data);
  } else {
    parser->last_error_offset = 0;
  }

  const char* reason = llhttp_get_error_reason(&parser->parser);
  if (reason == nullptr || reason[0] == '\0') {
    reason = llhttp_errno_name(err);
  }
  parser->last_error_reason = reason != nullptr ? reason : "";
}

napi_value LlhttpAllocCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 2 || argv[1] == nullptr) {
    napi_throw_type_error(env, nullptr, "llhttp_alloc requires parser type and JS parser");
    return nullptr;
  }

  int32_t type = 0;
  if (napi_get_value_int32(env, argv[0], &type) != napi_ok) {
    napi_throw_type_error(env, nullptr, "Invalid llhttp parser type");
    return nullptr;
  }

  auto* parser = new NativeLlhttpParser(env);
  llhttp_settings_init(&parser->settings);
  parser->settings.on_message_begin = OnMessageBegin;
  parser->settings.on_status = OnStatus;
  parser->settings.on_header_field = OnHeaderField;
  parser->settings.on_header_value = OnHeaderValue;
  parser->settings.on_headers_complete = OnHeadersComplete;
  parser->settings.on_body = OnBody;
  parser->settings.on_message_complete = OnMessageComplete;
  llhttp_init(&parser->parser, static_cast<llhttp_type_t>(type), &parser->settings);
  parser->parser.data = parser;

  if (napi_create_reference(env, argv[1], 1, &parser->parser_ref) != napi_ok ||
      parser->parser_ref == nullptr) {
    delete parser;
    return nullptr;
  }

  auto* handle = new NativeLlhttpHandle();
  handle->parser = parser;
  napi_value out = nullptr;
  if (napi_create_external(env, handle, NativeLlhttpHandleFinalize, nullptr, &out) != napi_ok ||
      out == nullptr) {
    delete handle;
    delete parser;
    return nullptr;
  }
  return out;
}

napi_value LlhttpExecuteCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 2) {
    napi_throw_type_error(env, nullptr, "llhttp_execute requires parser handle and chunk");
    return nullptr;
  }
  NativeLlhttpParser* parser = GetNativeParser(env, argv[0]);
  if (parser == nullptr) return nullptr;

  const char* data = nullptr;
  size_t length = 0;
  if (!ReadBufferLike(env, argv[1], &data, &length)) {
    napi_throw_type_error(env, nullptr, "llhttp_execute chunk must be a Buffer or Uint8Array");
    return nullptr;
  }

  parser->pending_exception = false;
  parser->current_data = data;
  parser->current_length = length;
  parser->last_error_offset = 0;
  parser->last_error_reason.clear();

  llhttp_errno_t err = llhttp_execute(&parser->parser, data, length);
  if (err != HPE_OK) {
    CaptureLlhttpError(parser, err);
  }

  parser->current_data = nullptr;
  parser->current_length = 0;

  if (parser->pending_exception) {
    return nullptr;
  }

  napi_value out = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(err), &out) != napi_ok) return nullptr;
  return out;
}

napi_value LlhttpResumeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  NativeLlhttpParser* parser = argc >= 1 ? GetNativeParser(env, argv[0]) : nullptr;
  if (parser == nullptr) return nullptr;
  llhttp_resume(&parser->parser);
  return Undefined(env);
}

napi_value LlhttpFreeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  void* raw = nullptr;
  if (napi_get_value_external(env, argv[0], &raw) != napi_ok || raw == nullptr) {
    return Undefined(env);
  }
  auto* handle = static_cast<NativeLlhttpHandle*>(raw);
  delete handle->parser;
  handle->parser = nullptr;
  return Undefined(env);
}

napi_value LlhttpGetErrorPosCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  NativeLlhttpParser* parser = argc >= 1 ? GetNativeParser(env, argv[0]) : nullptr;
  if (parser == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_create_uint32(env, static_cast<uint32_t>(parser->last_error_offset), &out) != napi_ok) return nullptr;
  return out;
}

napi_value LlhttpGetErrorReasonStringCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  NativeLlhttpParser* parser = argc >= 1 ? GetNativeParser(env, argv[0]) : nullptr;
  if (parser == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_create_string_utf8(
          env, parser->last_error_reason.c_str(), parser->last_error_reason.size(), &out) != napi_ok) {
    return nullptr;
  }
  return out;
}

napi_value GetCachedUndiciBinding(napi_env env) {
  auto* state = GetUndiciBindingState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  return GetRefValue(env, state->binding_ref);
}

}  // namespace

napi_value InitUndici(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedUndiciBinding(env);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  napi_value llhttp = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr ||
      napi_create_object(env, &llhttp) != napi_ok || llhttp == nullptr) {
    return undefined;
  }

  if (!SetBool(env, llhttp, "native", true) ||
      !SetNamedFunction(env, llhttp, "llhttp_alloc", LlhttpAllocCallback) ||
      !SetNamedFunction(env, llhttp, "llhttp_execute", LlhttpExecuteCallback) ||
      !SetNamedFunction(env, llhttp, "llhttp_resume", LlhttpResumeCallback) ||
      !SetNamedFunction(env, llhttp, "llhttp_free", LlhttpFreeCallback) ||
      !SetNamedFunction(env, llhttp, "llhttp_get_error_pos", LlhttpGetErrorPosCallback) ||
      !SetNamedFunction(env, llhttp, "llhttp_get_error_reason_string", LlhttpGetErrorReasonStringCallback) ||
      napi_set_named_property(env, binding, "llhttp", llhttp) != napi_ok) {
    return undefined;
  }

  auto& state = EnsureUndiciBindingState(env);
  DeleteRefIfPresent(env, &state.binding_ref);
  if (napi_create_reference(env, binding, 1, &state.binding_ref) != napi_ok) {
    state.binding_ref = nullptr;
  }
  return binding;
}

}  // namespace internal_binding
