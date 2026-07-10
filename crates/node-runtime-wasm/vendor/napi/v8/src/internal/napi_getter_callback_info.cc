#include "internal/napi_getter_callback_info.h"

napi_getter_callback_info__::napi_getter_callback_info__(
    napi_env env,
    void* data,
    const v8::PropertyCallbackInfo<v8::Value>& info)
    : env_(env), data_(data), info_(info) {}

size_t napi_getter_callback_info__::argc() const {
  return 0;
}

void napi_getter_callback_info__::args(napi_value* argv, size_t argc) const {
  napi_value undefined = napi_v8_wrap_value(env_, v8::Undefined(env_->isolate));
  for (size_t i = 0; i < argc; ++i) {
    argv[i] = undefined;
  }
}

napi_value napi_getter_callback_info__::this_arg() const {
  return napi_v8_wrap_value(env_, info_.This());
}

napi_value napi_getter_callback_info__::new_target() const {
  return nullptr;
}

void* napi_getter_callback_info__::data() const {
  return data_;
}
