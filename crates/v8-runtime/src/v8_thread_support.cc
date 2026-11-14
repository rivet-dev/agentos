#include "v8-platform.h"
#include "v8-sandbox.h"

extern "C" void agentos_v8_initialize_sandbox_hardware_before_thread_creation() {
  v8::SandboxHardwareSupport::InitializeBeforeThreadCreation();
}

extern "C" void agentos_v8_set_default_thread_isolation_permissions() {
  v8::ThreadIsolatedAllocator::SetDefaultPermissionsForSignalHandler();
}

#include "v8-isolate.h"
#include "v8-primitive.h"

// A two-byte script source that outlives every isolate. Its address is in
// the snapshot's external reference table, so restored isolates share it.
namespace {
class AgentosStaticTwoByteResource final
    : public v8::String::ExternalStringResource {
 public:
  AgentosStaticTwoByteResource(const uint16_t* data, size_t length)
      : data_(data), length_(length) {}
  const uint16_t* data() const override { return data_; }
  size_t length() const override { return length_; }

 protected:
  // Static: never freed when a string or isolate goes away.
  void Dispose() override {}

 private:
  const uint16_t* data_;
  size_t length_;
};
}  // namespace

extern "C" void* agentos_v8_static_twobyte_resource(const uint16_t* data,
                                                    size_t length) {
  return new AgentosStaticTwoByteResource(data, length);
}

extern "C" const v8::String* agentos_v8_new_static_twobyte_string(
    v8::Isolate* isolate, void* resource) {
  v8::Local<v8::String> string;
  if (!v8::String::NewExternalTwoByte(
           isolate, static_cast<AgentosStaticTwoByteResource*>(resource))
           .ToLocal(&string)) {
    return nullptr;
  }
  return *string;
}
