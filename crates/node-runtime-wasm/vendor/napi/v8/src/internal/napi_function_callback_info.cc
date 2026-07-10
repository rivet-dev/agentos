#include "internal/napi_function_callback_info.h"

napi_function_callback_info__::napi_function_callback_info__(
    const v8::FunctionCallbackInfo<v8::Value>& info,
    napi_callback_payload__* payload)
    : info_(info), payload_(payload) {}

size_t napi_function_callback_info__::argc() const {
  return static_cast<size_t>(info_.Length());
}

void napi_function_callback_info__::args(napi_value* argv, size_t argc) const {
  size_t i = 0;
  const size_t actual_argc = this->argc();
  const size_t copied_argc = (argc < actual_argc) ? argc : actual_argc;
  for (; i < copied_argc; ++i) {
    argv[i] = napi_v8_wrap_value(payload_->env, info_[static_cast<int>(i)]);
  }
  if (i < argc) {
    napi_value undefined =
        napi_v8_wrap_value(payload_->env, v8::Undefined(info_.GetIsolate()));
    for (; i < argc; ++i) {
      argv[i] = undefined;
    }
  }
}

napi_value napi_function_callback_info__::this_arg() const {
  return napi_v8_wrap_value(payload_->env, info_.This());
}

napi_value napi_function_callback_info__::new_target() const {
  if (info_.IsConstructCall()) {
    return napi_v8_wrap_value(payload_->env, info_.NewTarget());
  }
  return nullptr;
}

void* napi_function_callback_info__::data() const {
  return payload_->data;
}
