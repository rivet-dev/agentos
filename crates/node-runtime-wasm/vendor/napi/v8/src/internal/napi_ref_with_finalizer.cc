#include "internal/napi_ref_with_finalizer.h"

#include <new>

#include "internal/napi_v8_env.h"

napi_ref_with_finalizer__* napi_ref_with_finalizer__::New(
    napi_env__* env,
    v8::Local<v8::Value> value,
    uint32_t initial_refcount,
    napi_ref_ownership__ ownership,
    node_api_basic_finalize finalize_cb,
    void* finalize_data,
    void* finalize_hint) {
  napi_ref_with_finalizer__* reference =
      env->allocate<napi_ref_with_finalizer__>(
          env,
          value,
          initial_refcount,
          ownership,
          finalize_cb,
          finalize_data,
          finalize_hint);
  if (reference != nullptr) {
    reference->Link(&env->finalizing_reflist);
  }
  return reference;
}

napi_ref_with_finalizer__::napi_ref_with_finalizer__(
    napi_env__* env,
    v8::Local<v8::Value> value,
    uint32_t initial_refcount,
    napi_ref_ownership__ ownership,
    node_api_basic_finalize finalize_cb,
    void* finalize_data,
    void* finalize_hint)
    : napi_ref__(env, value, initial_refcount, ownership),
      finalize_cb_(finalize_cb),
      finalize_data_(finalize_data),
      finalize_hint_(finalize_hint) {}

napi_ref_with_finalizer__::~napi_ref_with_finalizer__() {
  if (env_ != nullptr) {
    env_->DequeueFinalizer(this);
  }
  ResetFinalizer();
}

void napi_ref_with_finalizer__::ResetFinalizer() {
  finalize_cb_ = nullptr;
  finalize_data_ = nullptr;
  finalize_hint_ = nullptr;
}

void napi_ref_with_finalizer__::Destroy() {
  napi_env__* env = env_;
  if (env != nullptr) {
    env->release(this);
  }
}

void napi_ref_with_finalizer__::CallUserFinalizer() {
  node_api_basic_finalize cb = finalize_cb_;
  void* cb_data = finalize_data_;
  void* cb_hint = finalize_hint_;
  ResetFinalizer();
  if (cb != nullptr) {
    if (env_ != nullptr) {
      env_->CallFinalizer(cb, cb_data, cb_hint);
    } else {
      cb(nullptr, cb_data, cb_hint);
    }
  }
}

void napi_ref_with_finalizer__::InvokeFinalizerFromGC() {
  if (env_ != nullptr) {
    env_->InvokeFinalizerFromGC(this);
  } else {
    Finalize();
  }
}
