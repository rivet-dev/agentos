#ifndef NAPI_V8_REF_WITH_DATA_H_
#define NAPI_V8_REF_WITH_DATA_H_

#include "internal/napi_ref.h"

struct napi_ref_with_data__ final : public napi_ref__ {
  static napi_ref_with_data__* New(napi_env__* env,
                                   v8::Local<v8::Value> value,
                                   uint32_t initial_refcount,
                                   napi_ref_ownership__ ownership,
                                   void* data);

  void* Data() const override { return data_; }
  void Destroy() override;

  napi_ref_with_data__(napi_env__* env,
                       v8::Local<v8::Value> value,
                       uint32_t initial_refcount,
                       napi_ref_ownership__ ownership,
                       void* data);

 private:
  void* data_ = nullptr;
};

#endif  // NAPI_V8_REF_WITH_DATA_H_
