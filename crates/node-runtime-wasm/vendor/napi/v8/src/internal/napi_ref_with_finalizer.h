#ifndef NAPI_V8_REF_WITH_FINALIZER_H_
#define NAPI_V8_REF_WITH_FINALIZER_H_

#include "internal/napi_ref.h"

struct napi_ref_with_finalizer__ final : public napi_ref__ {
  static napi_ref_with_finalizer__* New(napi_env__* env,
                                        v8::Local<v8::Value> value,
                                        uint32_t initial_refcount,
                                        napi_ref_ownership__ ownership,
                                        node_api_basic_finalize finalize_cb,
                                        void* finalize_data,
                                        void* finalize_hint);
  ~napi_ref_with_finalizer__() override;

  void* Data() const override { return finalize_data_; }
  void ResetFinalizer() override;
  void Destroy() override;

  napi_ref_with_finalizer__(napi_env__* env,
                            v8::Local<v8::Value> value,
                            uint32_t initial_refcount,
                            napi_ref_ownership__ ownership,
                            node_api_basic_finalize finalize_cb,
                            void* finalize_data,
                            void* finalize_hint);

 private:
  void CallUserFinalizer() override;
  void InvokeFinalizerFromGC() override;

  node_api_basic_finalize finalize_cb_ = nullptr;
  void* finalize_data_ = nullptr;
  void* finalize_hint_ = nullptr;
};

#endif  // NAPI_V8_REF_WITH_FINALIZER_H_
