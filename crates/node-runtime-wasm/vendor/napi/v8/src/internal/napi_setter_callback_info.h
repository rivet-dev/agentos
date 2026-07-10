#ifndef NAPI_V8_SETTER_CALLBACK_INFO_H_
#define NAPI_V8_SETTER_CALLBACK_INFO_H_

#include <v8.h>

#include "internal/napi_v8_env.h"

struct napi_setter_callback_info__ : public napi_callback_info__ {
  napi_setter_callback_info__(
      napi_env env,
      void* data,
      v8::Local<v8::Value> value,
      const v8::PropertyCallbackInfo<void>& info);

  size_t argc() const override;
  void args(napi_value* argv, size_t argc) const override;
  napi_value this_arg() const override;
  napi_value new_target() const override;
  void* data() const override;

 private:
  napi_env env_ = nullptr;
  void* data_ = nullptr;
  v8::Local<v8::Value> value_;
  const v8::PropertyCallbackInfo<void>& info_;
};

#endif  // NAPI_V8_SETTER_CALLBACK_INFO_H_
