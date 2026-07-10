#include "internal/napi_external_wrapper.h"

#include <new>

#include "internal/napi_v8_env.h"

v8::Local<v8::External> napi_external_wrapper__::New(napi_env__* env,
                                                     void* data) {
  auto* wrapper = env->allocate<napi_external_wrapper__>(env, data);
  if (wrapper == nullptr) {
    return v8::Local<v8::External>();
  }
  v8::Local<v8::External> external = v8::External::New(env->isolate, wrapper);
  wrapper->handle_.Reset(env->isolate, external);
  wrapper->handle_.SetWeak(wrapper, WeakCallback, v8::WeakCallbackType::kParameter);
  return external;
}

napi_external_wrapper__* napi_external_wrapper__::From(
    v8::Local<v8::External> external) {
  return static_cast<napi_external_wrapper__*>(external->Value());
}

bool napi_external_wrapper__::TypeTag(const napi_type_tag* tag) {
  if (has_type_tag_) return false;
  type_tag_ = *tag;
  has_type_tag_ = true;
  return true;
}

bool napi_external_wrapper__::CheckTypeTag(const napi_type_tag* tag) const {
  return has_type_tag_ && tag->lower == type_tag_.lower &&
         tag->upper == type_tag_.upper;
}

void napi_external_wrapper__::WeakCallback(
    const v8::WeakCallbackInfo<napi_external_wrapper__>& info) {
  napi_external_wrapper__* wrapper = info.GetParameter();
  if (wrapper != nullptr && wrapper->env_ != nullptr) {
    wrapper->env_->release(wrapper);
  }
}
