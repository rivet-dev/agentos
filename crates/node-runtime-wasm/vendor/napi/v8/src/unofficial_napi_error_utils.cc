#include "unofficial_napi_error_utils.h"

#include <algorithm>
#include <mutex>
#include <string>
#include <unordered_map>

#include "node_api.h"

namespace {

v8::Local<v8::String> OneByteString(v8::Isolate* isolate, const char* value) {
  return v8::String::NewFromUtf8(isolate, value, v8::NewStringType::kInternalized)
      .ToLocalChecked();
}

v8::Local<v8::Private> ApiPrivate(v8::Isolate* isolate, const char* description) {
  return v8::Private::ForApi(isolate, OneByteString(isolate, description));
}

std::string V8ValueToUtf8(v8::Isolate* isolate, v8::Local<v8::Value> value) {
  if (value.IsEmpty()) return {};
  v8::String::Utf8Value utf8(isolate, value);
  if (*utf8 == nullptr) return {};
  return std::string(*utf8, utf8.length());
}

v8::Local<v8::Message> GetMessageFromError(napi_env env, napi_value error) {
  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty()) return v8::Local<v8::Message>();
  return v8::Exception::CreateMessage(isolate, raw);
}

struct ErrorFormattingState {
  bool source_maps_enabled = false;
  v8::Global<v8::Function> get_source_map_error_source;
  v8::Global<v8::Value> preserved_exception;
  std::string preserved_exception_line;
  std::string preserved_thrown_at;
};

std::mutex g_error_formatting_mu;
std::unordered_map<napi_env, ErrorFormattingState> g_error_formatting_states;

std::string FormatStackTrace(v8::Isolate* isolate, v8::Local<v8::StackTrace> stack) {
  if (stack.IsEmpty()) return {};

  std::string result;
  for (int i = 0; i < stack->GetFrameCount(); ++i) {
    v8::Local<v8::StackFrame> frame = stack->GetFrame(isolate, i);
    v8::String::Utf8Value fn_name_utf8(isolate, frame->GetFunctionName());
    v8::String::Utf8Value script_name_utf8(isolate, frame->GetScriptName());
    const std::string fn_name = *fn_name_utf8 != nullptr ? *fn_name_utf8 : "";
    const std::string script_name = *script_name_utf8 != nullptr ? *script_name_utf8 : "";
    const int line_number = frame->GetLineNumber();
    const int column = frame->GetColumn();

    if (frame->IsEval()) {
      if (frame->GetScriptId() == v8::Message::kNoScriptIdInfo) {
        result += "    at [eval]:" + std::to_string(line_number) + ":" +
                  std::to_string(column) + "\n";
      } else {
        result += "    at [eval] (" + script_name + ":" +
                  std::to_string(line_number) + ":" + std::to_string(column) +
                  ")\n";
      }
      break;
    }

    if (fn_name.empty()) {
      result += "    at " + script_name + ":" + std::to_string(line_number) +
                ":" + std::to_string(column) + "\n";
    } else {
      result += "    at " + fn_name + " (" + script_name + ":" +
                std::to_string(line_number) + ":" + std::to_string(column) +
                ")\n";
    }
  }

  return result;
}

std::string CallGetSourceMapErrorSource(v8::Isolate* isolate,
                                        v8::Local<v8::Context> context,
                                        v8::Local<v8::Function> callback,
                                        v8::Local<v8::Value> script_resource_name,
                                        int line_number,
                                        int column_number) {
  v8::TryCatch try_catch(isolate);
  v8::Local<v8::Value> argv[] = {
      script_resource_name,
      v8::Int32::New(isolate, line_number),
      v8::Int32::New(isolate, column_number),
  };
  v8::MaybeLocal<v8::Value> maybe_result =
      callback->Call(context, v8::Undefined(isolate), 3, argv);
  v8::Local<v8::Value> result;
  if (!maybe_result.ToLocal(&result) || !result->IsString()) {
    return {};
  }
  return V8ValueToUtf8(isolate, result);
}

std::string GetErrorSourceLineForStderrImpl(napi_env env, v8::Local<v8::Message> message) {
  if (env == nullptr || env->isolate == nullptr || message.IsEmpty()) return {};

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);
  v8::Local<v8::Context> context = env->context();

  v8::Local<v8::String> source_line;
  if (!message->GetSourceLine(context).ToLocal(&source_line)) {
    return {};
  }
  const std::string source_line_utf8 = V8ValueToUtf8(isolate, source_line);
  if (source_line_utf8.find("node-do-not-add-exception-line") != std::string::npos) {
    return source_line_utf8;
  }

  ErrorFormattingState state;
  {
    std::lock_guard<std::mutex> lock(g_error_formatting_mu);
    auto it = g_error_formatting_states.find(env);
    if (it != g_error_formatting_states.end()) {
      state.source_maps_enabled = it->second.source_maps_enabled;
      if (!it->second.get_source_map_error_source.IsEmpty()) {
        state.get_source_map_error_source.Reset(
            isolate, it->second.get_source_map_error_source.Get(isolate));
      }
    }
  }

  if (state.source_maps_enabled && !state.get_source_map_error_source.IsEmpty()) {
    v8::Local<v8::Value> script_resource_name = message->GetScriptResourceName();
    if (!script_resource_name.IsEmpty() && !script_resource_name->IsUndefined()) {
      const int line_number = message->GetLineNumber(context).FromMaybe(0);
      const int column_number = message->GetStartColumn(context).FromMaybe(0);
      const std::string mapped = CallGetSourceMapErrorSource(
          isolate,
          context,
          state.get_source_map_error_source.Get(isolate),
          script_resource_name,
          line_number,
          column_number);
      if (!mapped.empty()) {
        return mapped;
      }
    }
  }

  return unofficial_napi_internal::BuildSyntaxArrowMessage(isolate, context, message);
}

void ClearPreservedErrorFormatting(ErrorFormattingState* state) {
  if (state == nullptr) return;
  state->preserved_exception.Reset();
  state->preserved_exception_line.clear();
  state->preserved_thrown_at.clear();
}

std::string GetThrownAtString(v8::Isolate* isolate, v8::Local<v8::Message> message) {
  if (isolate == nullptr || message.IsEmpty()) return {};
  v8::Local<v8::StackTrace> stack = message->GetStackTrace();
  if (!stack.IsEmpty() && stack->GetFrameCount() > 0) {
    return "Thrown at:\n" + FormatStackTrace(isolate, stack);
  }

  v8::Local<v8::Context> context = isolate->GetCurrentContext();
  std::string script_name = "<anonymous>";
  v8::Local<v8::Value> script_resource_name = message->GetScriptResourceName();
  if (!script_resource_name.IsEmpty() && !script_resource_name->IsUndefined()) {
    const std::string utf8_name = V8ValueToUtf8(isolate, script_resource_name);
    if (!utf8_name.empty()) {
      script_name = utf8_name;
    }
  }

  const int line_number = message->GetLineNumber(context).FromMaybe(0);
  const int column_number = message->GetStartColumn(context).FromMaybe(0) + 1;
  if (line_number <= 0 || column_number <= 0) return {};

  return "Thrown at:\n    at " + script_name + ":" +
         std::to_string(line_number) + ":" + std::to_string(column_number) + "\n";
}

bool TakePreservedErrorFormattingImpl(napi_env env,
                                      v8::Local<v8::Value> raw,
                                      std::string* source_line_out,
                                      std::string* thrown_at_out) {
  if (env == nullptr || env->isolate == nullptr || raw.IsEmpty()) return false;

  std::lock_guard<std::mutex> lock(g_error_formatting_mu);
  auto it = g_error_formatting_states.find(env);
  if (it == g_error_formatting_states.end()) return false;

  ErrorFormattingState& state = it->second;
  if (state.preserved_exception.IsEmpty()) return false;

  v8::Local<v8::Value> preserved = state.preserved_exception.Get(env->isolate);
  if (preserved.IsEmpty() || !preserved->StrictEquals(raw)) return false;

  if (source_line_out != nullptr) *source_line_out = state.preserved_exception_line;
  if (thrown_at_out != nullptr) *thrown_at_out = state.preserved_thrown_at;
  ClearPreservedErrorFormatting(&state);
  return true;
}

}  // namespace

namespace unofficial_napi_internal {

std::string GetErrorSourceLineForStderrImpl(napi_env env,
                                            v8::Local<v8::Message> message) {
  return ::GetErrorSourceLineForStderrImpl(env, message);
}

std::string GetThrownAtString(v8::Isolate* isolate,
                              v8::Local<v8::Message> message) {
  return ::GetThrownAtString(isolate, message);
}

void PreserveErrorFormatting(napi_env env,
                             v8::Local<v8::Value> exception,
                             const std::string& source_line,
                             const std::string& thrown_at) {
  if (env == nullptr || env->isolate == nullptr || exception.IsEmpty()) return;

  std::lock_guard<std::mutex> lock(g_error_formatting_mu);
  ErrorFormattingState& state = g_error_formatting_states[env];
  ClearPreservedErrorFormatting(&state);
  state.preserved_exception.Reset(env->isolate, exception);
  state.preserved_exception_line = source_line;
  state.preserved_thrown_at = thrown_at;
}

std::string BuildSyntaxArrowMessage(v8::Isolate* isolate,
                                    v8::Local<v8::Context> context,
                                    v8::Local<v8::Message> message) {
  if (message.IsEmpty()) return {};

  std::string filename = "<anonymous_script>";
  v8::Local<v8::Value> script_resource_name = message->GetScriptResourceName();
  if (!script_resource_name.IsEmpty() && !script_resource_name->IsUndefined()) {
    const std::string utf8_name = V8ValueToUtf8(isolate, script_resource_name);
    if (!utf8_name.empty()) filename = utf8_name;
  }

  const int line_number = message->GetLineNumber(context).FromMaybe(0);
  v8::MaybeLocal<v8::String> source_line_maybe = message->GetSourceLine(context);
  v8::Local<v8::String> source_line_v8;
  if (!source_line_maybe.ToLocal(&source_line_v8)) return {};

  const std::string source_line = V8ValueToUtf8(isolate, source_line_v8);
  if (source_line.empty()) return {};

  int start = message->GetStartColumn(context).FromMaybe(0);
  int end = message->GetEndColumn(context).FromMaybe(start + 1);
  v8::ScriptOrigin origin = message->GetScriptOrigin();
  const int script_start =
      (line_number - origin.LineOffset()) == 1 ? origin.ColumnOffset() : 0;
  if (start >= script_start) {
    end -= script_start;
    start -= script_start;
  }
  if (end <= start) end = start + 1;
  if (start < 0) start = 0;
  if (end < 0) end = 0;

  std::string underline(static_cast<size_t>(start), ' ');
  underline.append(static_cast<size_t>(std::max(1, end - start)), '^');

  return filename + ":" + std::to_string(line_number) + "\n" +
         source_line + "\n" +
         underline + "\n\n";
}

void AttachSyntaxArrowMessage(v8::Isolate* isolate,
                              v8::Local<v8::Context> context,
                              v8::Local<v8::Value> exception,
                              v8::Local<v8::Message> message) {
  if (exception.IsEmpty() || !exception->IsObject() || message.IsEmpty()) return;

  v8::Local<v8::Object> err_obj = exception.As<v8::Object>();
  v8::Local<v8::Private> arrow_key = ApiPrivate(isolate, "node:arrowMessage");
  v8::Local<v8::Private> decorated_key = ApiPrivate(isolate, "node:decorated");

  SetArrowMessage(isolate, context, exception, message);

  v8::Local<v8::Value> decorated;
  const bool already_decorated =
      err_obj->GetPrivate(context, decorated_key).ToLocal(&decorated) && decorated->IsTrue();
  if (already_decorated) return;

  v8::Local<v8::Value> existing_arrow;
  if (!err_obj->GetPrivate(context, arrow_key).ToLocal(&existing_arrow) || !existing_arrow->IsString()) {
    return;
  }

  v8::Local<v8::Value> stack_value;
  if (!err_obj->Get(context, OneByteString(isolate, "stack")).ToLocal(&stack_value) || !stack_value->IsString()) {
    return;
  }

  v8::Local<v8::String> decorated_stack =
      v8::String::Concat(isolate, existing_arrow.As<v8::String>(), stack_value.As<v8::String>());
  if (!err_obj->Set(context, OneByteString(isolate, "stack"), decorated_stack).FromMaybe(false)) {
    return;
  }
  (void)err_obj->SetPrivate(context, decorated_key, v8::True(isolate));
}

void SetArrowMessageFromString(v8::Isolate* isolate,
                               v8::Local<v8::Context> context,
                               v8::Local<v8::Value> exception,
                               const std::string& arrow) {
  if (exception.IsEmpty() || !exception->IsObject() || arrow.empty()) return;
  v8::Local<v8::Object> err_obj = exception.As<v8::Object>();
  v8::Local<v8::Private> arrow_key = ApiPrivate(isolate, "node:arrowMessage");
  v8::Local<v8::Value> existing_arrow;
  if (err_obj->GetPrivate(context, arrow_key).ToLocal(&existing_arrow) && existing_arrow->IsString()) {
    return;
  }

  v8::Local<v8::String> arrow_v8;
  if (!v8::String::NewFromUtf8(isolate,
                               arrow.c_str(),
                               v8::NewStringType::kNormal,
                               static_cast<int>(arrow.size()))
           .ToLocal(&arrow_v8)) {
    return;
  }
  (void)err_obj->SetPrivate(context, arrow_key, arrow_v8);
}

void SetArrowMessage(v8::Isolate* isolate,
                     v8::Local<v8::Context> context,
                     v8::Local<v8::Value> exception,
                     v8::Local<v8::Message> message) {
  if (exception.IsEmpty() || !exception->IsObject() || message.IsEmpty()) return;

  SetArrowMessageFromString(
      isolate, context, exception, BuildSyntaxArrowMessage(isolate, context, message));
}

napi_status SetSourceMapsEnabled(napi_env env, bool enabled) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;
  std::lock_guard<std::mutex> lock(g_error_formatting_mu);
  g_error_formatting_states[env].source_maps_enabled = enabled;
  return napi_ok;
}

napi_status SetGetSourceMapErrorSourceCallback(napi_env env, napi_value callback) {
  if (env == nullptr || env->isolate == nullptr) return napi_invalid_arg;

  v8::Local<v8::Value> raw =
      callback != nullptr ? napi_v8_unwrap_value(callback) : v8::Local<v8::Value>();
  if (!raw.IsEmpty() && !raw->IsFunction()) return napi_invalid_arg;

  std::lock_guard<std::mutex> lock(g_error_formatting_mu);
  ErrorFormattingState& state = g_error_formatting_states[env];
  state.get_source_map_error_source.Reset();
  if (!raw.IsEmpty()) {
    state.get_source_map_error_source.Reset(env->isolate, raw.As<v8::Function>());
  }
  return napi_ok;
}

napi_status PreserveErrorSourceMessage(napi_env env, napi_value error) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr) {
    return napi_invalid_arg;
  }

  v8::Isolate* isolate = env->isolate;
  v8::HandleScope scope(isolate);
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty()) return napi_invalid_arg;

  v8::Local<v8::Message> message = v8::Exception::CreateMessage(isolate, raw);
  if (message.IsEmpty()) return napi_generic_failure;

  const std::string source_line = GetErrorSourceLineForStderrImpl(env, message);
  SetArrowMessageFromString(isolate, context, raw, source_line);

  PreserveErrorFormatting(env,
                          raw,
                          source_line,
                          GetThrownAtString(isolate, message));
  return napi_ok;
}

napi_status TakePreservedErrorFormatting(napi_env env,
                                         napi_value error,
                                         napi_value* source_line_out,
                                         napi_value* thrown_at_out) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr) {
    return napi_invalid_arg;
  }

  if (source_line_out != nullptr) *source_line_out = nullptr;
  if (thrown_at_out != nullptr) *thrown_at_out = nullptr;

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty()) return napi_invalid_arg;

  std::string preserved_source_line;
  std::string preserved_thrown_at;
  if (!TakePreservedErrorFormattingImpl(
          env, raw, &preserved_source_line, &preserved_thrown_at)) {
    return napi_ok;
  }

  if (source_line_out != nullptr && !preserved_source_line.empty()) {
    v8::Local<v8::String> source_line_value;
    if (!v8::String::NewFromUtf8(
             isolate,
             preserved_source_line.c_str(),
             v8::NewStringType::kNormal,
             static_cast<int>(preserved_source_line.size()))
             .ToLocal(&source_line_value)) {
      return napi_generic_failure;
    }
    *source_line_out = napi_v8_wrap_value(env, source_line_value);
    if (*source_line_out == nullptr) return napi_generic_failure;
  }

  if (thrown_at_out != nullptr && !preserved_thrown_at.empty()) {
    v8::Local<v8::String> thrown_at_value;
    if (!v8::String::NewFromUtf8(
             isolate,
             preserved_thrown_at.c_str(),
             v8::NewStringType::kNormal,
             static_cast<int>(preserved_thrown_at.size()))
             .ToLocal(&thrown_at_value)) {
      return napi_generic_failure;
    }
    *thrown_at_out = napi_v8_wrap_value(env, thrown_at_value);
    if (*thrown_at_out == nullptr) return napi_generic_failure;
  }

  return napi_ok;
}

void ResetErrorFormattingState(napi_env env) {
  if (env == nullptr) return;
  std::lock_guard<std::mutex> lock(g_error_formatting_mu);
  auto it = g_error_formatting_states.find(env);
  if (it == g_error_formatting_states.end()) return;
  ClearPreservedErrorFormatting(&it->second);
  it->second.get_source_map_error_source.Reset();
  g_error_formatting_states.erase(it);
}

napi_status GetErrorSourcePositions(napi_env env,
                                    napi_value error,
                                    unofficial_napi_error_source_positions* out) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr || out == nullptr) {
    return napi_invalid_arg;
  }

  out->source_line = nullptr;
  out->script_resource_name = nullptr;
  out->line_number = 0;
  out->start_column = 0;
  out->end_column = 0;

  v8::Isolate* isolate = env->isolate;
  v8::Local<v8::Context> context = env->context();
  v8::Local<v8::Value> raw = napi_v8_unwrap_value(error);
  if (raw.IsEmpty() || !raw->IsObject()) return napi_invalid_arg;

  v8::Local<v8::Message> msg = GetMessageFromError(env, error);
  if (msg.IsEmpty()) return napi_generic_failure;

  v8::Local<v8::String> source_line;
  if (!msg->GetSourceLine(context).ToLocal(&source_line)) {
    return napi_generic_failure;
  }

  int line_number = 0;
  if (!msg->GetLineNumber(context).To(&line_number)) {
    return napi_generic_failure;
  }

  out->source_line = napi_v8_wrap_value(env, source_line);
  if (out->source_line == nullptr) return napi_generic_failure;

  v8::Local<v8::Value> resource_name = msg->GetScriptOrigin().ResourceName();
  out->script_resource_name = napi_v8_wrap_value(env, resource_name);
  if (out->script_resource_name == nullptr) return napi_generic_failure;

  out->line_number = line_number;
  out->start_column = msg->GetStartColumn(context).FromMaybe(0);
  out->end_column = msg->GetEndColumn(context).FromMaybe(out->start_column + 1);
  return napi_ok;
}

napi_status GetErrorSourceLineForStderr(napi_env env,
                                        napi_value error,
                                        napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr ||
      result_out == nullptr) {
    return napi_invalid_arg;
  }

  *result_out = nullptr;
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope scope(isolate);
  v8::Local<v8::Message> message = GetMessageFromError(env, error);
  if (message.IsEmpty()) return napi_generic_failure;

  const std::string formatted = GetErrorSourceLineForStderrImpl(env, message);
  if (formatted.empty()) return napi_ok;

  v8::Local<v8::String> out;
  if (!v8::String::NewFromUtf8(
           isolate,
           formatted.c_str(),
           v8::NewStringType::kNormal,
           static_cast<int>(formatted.size()))
           .ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result_out = napi_v8_wrap_value(env, scope.Escape(out));
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

napi_status GetErrorThrownAt(napi_env env,
                             napi_value error,
                             napi_value* result_out) {
  if (env == nullptr || env->isolate == nullptr || error == nullptr ||
      result_out == nullptr) {
    return napi_invalid_arg;
  }

  *result_out = nullptr;
  v8::Isolate* isolate = env->isolate;
  v8::EscapableHandleScope scope(isolate);
  v8::Local<v8::Message> message = GetMessageFromError(env, error);
  if (message.IsEmpty()) return napi_generic_failure;

  v8::Local<v8::StackTrace> stack = message->GetStackTrace();
  if (stack.IsEmpty() || stack->GetFrameCount() == 0) return napi_ok;

  const std::string thrown_at = "Thrown at:\n" + FormatStackTrace(isolate, stack);
  v8::Local<v8::String> out;
  if (!v8::String::NewFromUtf8(
           isolate,
           thrown_at.c_str(),
           v8::NewStringType::kNormal,
           static_cast<int>(thrown_at.size()))
           .ToLocal(&out)) {
    return napi_generic_failure;
  }
  *result_out = napi_v8_wrap_value(env, scope.Escape(out));
  return *result_out == nullptr ? napi_generic_failure : napi_ok;
}

}  // namespace unofficial_napi_internal
