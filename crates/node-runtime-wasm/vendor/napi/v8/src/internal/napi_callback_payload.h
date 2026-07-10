#ifndef NAPI_V8_CALLBACK_PAYLOAD_H_
#define NAPI_V8_CALLBACK_PAYLOAD_H_

#include "js_native_api.h"

struct napi_callback_payload__ {
  napi_env env;
  napi_callback cb;
  void* data;
};

struct napi_accessor_payload__ {
  napi_env env;
  napi_callback getter_cb;
  napi_callback setter_cb;
  void* data;
};

#endif  // NAPI_V8_CALLBACK_PAYLOAD_H_
