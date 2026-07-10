#ifndef NAPI_V8_REF_H_
#define NAPI_V8_REF_H_

#include <cstdint>
#include <cstddef>

#include <v8.h>

#include "js_native_api.h"
#include "napi_allocator.h"
#include "internal/napi_ref_tracker.h"

struct napi_env__;

enum class napi_ref_ownership__ : uint8_t {
  kRuntime,
  kUserland,
};

struct napi_ref__ : public napi_ref_tracker__ {
  static napi_ref__* New(napi_env__* env,
                         v8::Local<v8::Value> value,
                         uint32_t initial_refcount,
                         napi_ref_ownership__ ownership);
  ~napi_ref__() override;

  uint32_t Ref();
  uint32_t Unref();
  v8::Local<v8::Value> Get() const;
  virtual void* Data() const { return nullptr; }
  virtual void ResetFinalizer() {}
  virtual void Destroy();
  void Invalidate();
  void Finalize() override;
  napi_ref_ownership__ ownership() const { return ownership_; }

  napi_ref__(napi_env__* env,
             v8::Local<v8::Value> value,
             uint32_t initial_refcount,
             napi_ref_ownership__ ownership);

 protected:
  virtual void CallUserFinalizer() {}
  virtual void InvokeFinalizerFromGC();
  napi_env__* env_ = nullptr;

 private:
  static void WeakCallback(const v8::WeakCallbackInfo<napi_ref__>& info);
  void SetWeak();

  v8::Global<v8::Value> value_;
  uint32_t refcount_ = 0;
  napi_ref_ownership__ ownership_ = napi_ref_ownership__::kUserland;
  bool can_be_weak_ = false;
};

#endif  // NAPI_V8_REF_H_
