#include <v8.h>
#include <cstdlib>
#include <cstring>
#include <limits>

// Rusty V8 builds its archive with its own C++ standard library and ABI.
// Keep all STL return types on that side of the boundary.
extern "C" void v8__ValueSerializer__Release(v8::ValueSerializer*, uint8_t**, size_t*);

// The pinned Rust V8 binding does not expose Delegate::ReallocateBufferMemory.
// Keep the allocation fuse here, inside V8's writer, rather than walking guest
// objects twice (getters can change them between a preflight and serialization).
namespace {
template <class T> v8::Local<T> from_handle(const T* handle) {
  static_assert(sizeof(v8::Local<T>) == sizeof(handle));
  v8::Local<T> local;
  std::memcpy(&local, &handle, sizeof(local));
  return local;
}

using Admit = bool (*)(void*, size_t);
class BoundedSerializer final : public v8::ValueSerializer::Delegate {
 public:
  BoundedSerializer(v8::Isolate* isolate, size_t limit, Admit admit, void* budget)
      : isolate_(isolate), limit_(limit), admit_(admit), budget_(budget), writer(isolate, this) {}

  void ThrowDataCloneError(v8::Local<v8::String> message) override {
    isolate_->ThrowException(v8::Exception::Error(message));
  }

  void* ReallocateBufferMemory(void* previous, size_t size, size_t* actual) override {
    // V8 requests geometric spare capacity (2 * old capacity + 64).
    // Keep logical wire limits unchanged while bounding that temporary growth.
    const size_t allocation_limit = limit_ > (SIZE_MAX - 64) / 2
        ? SIZE_MAX : limit_ * 2 + 64;
    if (size > allocation_limit || size > SIZE_MAX - capacity ||
        !admit_(budget_, capacity + size)) {
      rejected = size;
      *actual = 0;
      return nullptr;
    }
    void* allocation = std::realloc(previous, size);
    *actual = allocation ? size : 0;
    if (!allocation) allocation_failed = true;
    else capacity = size;
    return allocation;
  }
  void FreeBufferMemory(void* buffer) override { std::free(buffer); }

  v8::Maybe<bool> WriteHostObject(v8::Isolate* isolate, v8::Local<v8::Object> object) override {
    if (!object->IsArrayBufferView()) return fail("Unsupported serialized host object");
    auto view = object.As<v8::ArrayBufferView>();
    auto context = isolate->GetCurrentContext();
    v8::Local<v8::Value> constructor, buffer;
    if (!object->Get(context, v8::String::NewFromUtf8Literal(isolate, "constructor")).ToLocal(&constructor) ||
        !context->Global()->Get(context, v8::String::NewFromUtf8Literal(isolate, "Buffer")).ToLocal(&buffer)) {
      return v8::Nothing<bool>();
    }
    // All guest getters have completed before capturing the backing store and
    // length. No guest code runs between this check and WriteRawBytes.
    auto array_buffer = view->Buffer();
    if (array_buffer->WasDetached()) return fail("Cannot serialize a detached ArrayBufferView");
    const size_t length = view->ByteLength();
    if (length > limit_ || length > std::numeric_limits<uint32_t>::max()) {
      rejected = length;
      return fail("Serialized view exceeds the configured byte limit");
    }
    uint32_t kind;
    if (object->IsUint8Array() && constructor->StrictEquals(buffer)) kind = 10;
    else if (object->IsInt8Array()) kind = 0;
    else if (object->IsUint8Array()) kind = 1;
    else if (object->IsUint8ClampedArray()) kind = 2;
    else if (object->IsInt16Array()) kind = 3;
    else if (object->IsUint16Array()) kind = 4;
    else if (object->IsInt32Array()) kind = 5;
    else if (object->IsUint32Array()) kind = 6;
    else if (object->IsFloat32Array()) kind = 7;
    else if (object->IsFloat64Array()) kind = 8;
    else if (object->IsDataView()) kind = 9;
    else if (object->IsBigInt64Array()) kind = 11;
    else if (object->IsBigUint64Array()) kind = 12;
    else if (object->IsFloat16Array()) kind = 13;
    else return fail("Unsupported serialized view");
    // The Local<ArrayBuffer> keeps the V8 allocation alive. Data and ByteLength
    // are scalar ABI calls. GetBackingStore returns an ABI-dependent shared_ptr.
    const size_t buffer_length = array_buffer->ByteLength();
    const size_t offset = view->ByteOffset();
    if (offset > buffer_length || length > buffer_length - offset)
      return fail("Serialized view changed before copying");
    writer.WriteUint32(kind);
    writer.WriteUint32(static_cast<uint32_t>(length));
    if (length) writer.WriteRawBytes(static_cast<const uint8_t*>(array_buffer->Data()) + offset, length);
    return v8::Just(true);
  }

  v8::Maybe<bool> fail(const char* message) {
    ThrowDataCloneError(v8::String::NewFromUtf8(isolate_, message).ToLocalChecked());
    return v8::Nothing<bool>();
  }
  v8::Isolate* isolate_;
  size_t limit_;
  Admit admit_;
  void* budget_;
  v8::ValueSerializer writer;
  size_t capacity = 0;
  size_t rejected = 0;
  bool allocation_failed = false;
};
}

extern "C" bool agentos_v8_serialize_bounded(
    const v8::Context* context_handle, const v8::Value* value_handle,
    size_t limit, Admit admit, void* budget, uint8_t** bytes, size_t* length,
    size_t* rejected, bool* allocation_failed) {
  auto context = from_handle(context_handle);
  auto value = from_handle(value_handle);
  // Rust enters this context on the owning isolate thread before making the call.
  BoundedSerializer serializer(v8::Isolate::GetCurrent(), limit, admit, budget);
  serializer.writer.SetTreatArrayBufferViewsAsHostObjects(true);
  serializer.writer.WriteHeaderForNodeWireFormat15();
  const bool ok = serializer.writer.WriteValue(context, value).FromMaybe(false);
  *rejected = serializer.rejected;
  *allocation_failed = serializer.allocation_failed;
  if (!ok || serializer.rejected || serializer.allocation_failed) return false;
  uint8_t* released_bytes = nullptr;
  size_t released_length = 0;
  v8__ValueSerializer__Release(&serializer.writer, &released_bytes, &released_length);
  if (released_length > limit) {
    *rejected = released_length;
    std::free(released_bytes);
    return false;
  }
  // Transfer exactly the logical bytes, not unused capacity, to ArrayBuffer.
  if (released_length > SIZE_MAX - serializer.capacity ||
      !admit(budget, serializer.capacity + released_length)) {
    *rejected = released_length;
    std::free(released_bytes);
    return false;
  }
  void* exact = std::realloc(released_bytes, released_length);
  if (!exact) {
    std::free(released_bytes);
    *allocation_failed = true;
    return false;
  }
  *bytes = static_cast<uint8_t*>(exact);
  *length = released_length;
  return true;
}
