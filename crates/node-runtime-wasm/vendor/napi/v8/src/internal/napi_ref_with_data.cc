#include "internal/napi_ref_with_data.h"

#include <new>

#include "internal/napi_v8_env.h"

napi_ref_with_data__* napi_ref_with_data__::New(napi_env__* env,
                                                v8::Local<v8::Value> value,
                                                uint32_t initial_refcount,
                                                napi_ref_ownership__ ownership,
                                                void* data) {
  napi_ref_with_data__* reference = env->allocate<napi_ref_with_data__>(
      env, value, initial_refcount, ownership, data);
  if (reference != nullptr) {
    reference->Link(&env->reflist);
  }
  return reference;
}

napi_ref_with_data__::napi_ref_with_data__(napi_env__* env,
                                           v8::Local<v8::Value> value,
                                           uint32_t initial_refcount,
                                           napi_ref_ownership__ ownership,
                                           void* data)
    : napi_ref__(env, value, initial_refcount, ownership), data_(data) {}

void napi_ref_with_data__::Destroy() {
  napi_env__* env = env_;
  if (env != nullptr) {
    env->release(this);
  }
}
