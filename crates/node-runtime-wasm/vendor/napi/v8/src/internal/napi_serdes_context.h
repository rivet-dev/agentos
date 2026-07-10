#ifndef NAPI_V8_SERDES_CONTEXT_H_
#define NAPI_V8_SERDES_CONTEXT_H_

#include <cstdint>
#include <memory>
#include <vector>

#include <v8.h>

#include "js_native_api.h"

namespace v8impl::detail {

class SerializerContext : public v8::ValueSerializer::Delegate {
 public:
  SerializerContext(napi_env env, v8::Local<v8::Object> wrap);
  ~SerializerContext() override;

  void ThrowDataCloneError(v8::Local<v8::String> message) override;
  v8::Maybe<uint32_t> GetSharedArrayBufferId(
      v8::Isolate* isolate,
      v8::Local<v8::SharedArrayBuffer> shared_array_buffer) override;
  v8::Maybe<bool> WriteHostObject(v8::Isolate* isolate,
                                  v8::Local<v8::Object> input) override;

  static void New(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteHeader(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteValue(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void SetTreatArrayBufferViewsAsHostObjects(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReleaseBuffer(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void TransferArrayBuffer(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteUint32(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteUint64(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteDouble(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WriteRawBytes(const v8::FunctionCallbackInfo<v8::Value>& args);

 private:
  static SerializerContext* Unwrap(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WeakCallback(
      const v8::WeakCallbackInfo<SerializerContext>& info);

  napi_env env_ = nullptr;
  v8::Isolate* isolate_ = nullptr;
  v8::Global<v8::Object> wrap_;
  v8::ValueSerializer serializer_;
};

class DeserializerContext : public v8::ValueDeserializer::Delegate {
 public:
  DeserializerContext(napi_env env,
                      v8::Local<v8::Object> wrap,
                      v8::Local<v8::Value> buffer);
  ~DeserializerContext() override;

  v8::MaybeLocal<v8::Object> ReadHostObject(v8::Isolate* isolate) override;

  static void New(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadHeader(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadValue(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void TransferArrayBuffer(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void GetWireFormatVersion(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadUint32(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadUint64(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadDouble(const v8::FunctionCallbackInfo<v8::Value>& args);
  static void ReadRawBytes(const v8::FunctionCallbackInfo<v8::Value>& args);

 private:
  static DeserializerContext* Unwrap(
      const v8::FunctionCallbackInfo<v8::Value>& args);
  static void WeakCallback(
      const v8::WeakCallbackInfo<DeserializerContext>& info);

  napi_env env_ = nullptr;
  v8::Isolate* isolate_ = nullptr;
  v8::Global<v8::Object> wrap_;
  std::vector<uint8_t> data_;
  std::unique_ptr<v8::ValueDeserializer> deserializer_;
};

}  // namespace v8impl::detail

#endif  // NAPI_V8_SERDES_CONTEXT_H_
