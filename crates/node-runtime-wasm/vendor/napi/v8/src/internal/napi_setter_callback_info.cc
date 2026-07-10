#include "internal/napi_setter_callback_info.h"

napi_setter_callback_info__::napi_setter_callback_info__(
    napi_env env,
    void* data,
    v8::Local<v8::Value> value,
    const v8::PropertyCallbackInfo<void>& info)
    : env_(env), data_(data), value_(value), info_(info) {}

size_t napi_setter_callback_info__::argc() const {
  return 1;
}

void napi_setter_callback_info__::args(napi_value* argv, size_t argc) const {
  if (argc == 0) return;
  argv[0] = napi_v8_wrap_value(env_, value_);
  napi_value undefined = napi_v8_wrap_value(env_, v8::Undefined(env_->isolate));
  for (size_t i = 1; i < argc; ++i) {
    argv[i] = undefined;
  }
}

napi_value napi_setter_callback_info__::this_arg() const {
  return napi_v8_wrap_value(env_, info_.This());
}

napi_value napi_setter_callback_info__::new_target() const {
  return nullptr;
}

void* napi_setter_callback_info__::data() const {
  return data_;
}
