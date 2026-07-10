#ifndef NAPI_V8_FUNCTION_CALLBACK_INFO_H_
#define NAPI_V8_FUNCTION_CALLBACK_INFO_H_

#include <v8.h>

#include "internal/napi_callback_payload.h"
#include "internal/napi_v8_env.h"

struct napi_function_callback_info__ : public napi_callback_info__ {
  napi_function_callback_info__(
      const v8::FunctionCallbackInfo<v8::Value>& info,
      napi_callback_payload__* payload);

  size_t argc() const override;
  void args(napi_value* argv, size_t argc) const override;
  napi_value this_arg() const override;
  napi_value new_target() const override;
  void* data() const override;

 private:
  const v8::FunctionCallbackInfo<v8::Value>& info_;
  napi_callback_payload__* payload_;
};

#endif  // NAPI_V8_FUNCTION_CALLBACK_INFO_H_
