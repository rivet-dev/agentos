#ifndef NAPI_V8_EXTERNAL_WRAPPER_H_
#define NAPI_V8_EXTERNAL_WRAPPER_H_

#include <v8.h>

#include "js_native_api.h"
#include "napi_allocator.h"

struct napi_env__;

struct napi_external_wrapper__ {
  static v8::Local<v8::External> New(napi_env__* env, void* data);
  static napi_external_wrapper__* From(v8::Local<v8::External> external);

  void* Data() const { return data_; }
  bool TypeTag(const napi_type_tag* tag);
  bool CheckTypeTag(const napi_type_tag* tag) const;

  napi_external_wrapper__(napi_env__* env, void* data) : env_(env), data_(data) {}

 private:
  static void WeakCallback(
      const v8::WeakCallbackInfo<napi_external_wrapper__>& info);

  napi_env__* env_ = nullptr;
  void* data_ = nullptr;
  napi_type_tag type_tag_{};
  bool has_type_tag_ = false;
  v8::Global<v8::Value> handle_;
};

#endif  // NAPI_V8_EXTERNAL_WRAPPER_H_
