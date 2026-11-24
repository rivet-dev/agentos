#if defined(__linux__) && !defined(_GNU_SOURCE)
#define _GNU_SOURCE 1
#endif

#include <cerrno>
#if defined(__linux__) && defined(__x86_64__)
#include <sys/mman.h>
#if defined(PKEY_DISABLE_WRITE)
#pragma weak pkey_set
#endif
#endif

#include "v8-platform.h"
#include "v8-sandbox.h"

extern "C" void agentos_v8_initialize_sandbox_hardware_before_thread_creation() {
  v8::SandboxHardwareSupport::InitializeBeforeThreadCreation();
}

extern "C" int agentos_v8_set_default_thread_isolation_permissions(v8::Platform* platform) {
  // The platform owns this allocator for its lifetime. A kernel without PKU
  // gets a null allocator. Never change keys owned by a different embedder.
  auto* allocator = platform->GetThreadIsolatedAllocator();
  if (!allocator) return 0;
#if defined(__linux__) && defined(__x86_64__) && defined(PKEY_DISABLE_WRITE)
  if (allocator->Type() == v8::ThreadIsolatedAllocator::Type::kPkey && pkey_set) {
    // Match the access rights used by V8's DefaultThreadIsolatedAllocator
    // when it allocates its key. This also prepares pre-existing host threads.
    const int key = allocator->Pkey();
    if (key <= 0) return EINVAL;
    return pkey_set(key, PKEY_DISABLE_WRITE) == 0 ? 0 : errno;
  }
#endif
  return ENOTSUP;
}


#include "v8-isolate.h"
#include "v8-primitive.h"
#include <cstddef>
#include <new>

using AgentOsSuppressMicrotasks = v8::Isolate::SuppressMicrotaskExecutionScope;
static_assert(sizeof(AgentOsSuppressMicrotasks) == 3 * sizeof(void*));
static_assert(alignof(AgentOsSuppressMicrotasks) <= alignof(void*));

extern "C" void agentos_v8_suppress_microtasks_begin(void* storage, v8::Isolate* isolate) {
  // The Rust caller pins storage BEFORE placement construction. nullptr selects
  // the default queue; call depth suppresses implicit checkpoints for all queues,
  // while an owned queue's explicit checkpoints remain available.
  // Global placement new constructs in that existing Rust stack slot; Clang
  // correctly deletes the V8 class allocation operators for stack-only scopes.
  ::new (storage) AgentOsSuppressMicrotasks(isolate, nullptr);
}

extern "C" void agentos_v8_suppress_microtasks_end(void* storage) {
  static_cast<AgentOsSuppressMicrotasks*>(storage)->~AgentOsSuppressMicrotasks();
}


#include <cstdint>
#include <limits>
#if defined(__linux__) || defined(__APPLE__)
#include <pthread.h>
#endif

#if defined(__has_feature)
#if __has_feature(address_sanitizer)
#define AGENTOS_TERMINAL_STACK_ASAN 1
#endif
#endif
#if defined(__SANITIZE_ADDRESS__)
#define AGENTOS_TERMINAL_STACK_ASAN 1
#endif

// Use the current thread's OS-reported bounds; never alter native stack mappings.
struct AgentOsTerminalStackPlan {
  uintptr_t stack_low;
  uintptr_t stack_high;
  uintptr_t guard_bytes;
  uintptr_t setup_sp;
  uintptr_t ordinary_limit;
  uintptr_t terminal_limit;
};

extern "C" int agentos_v8_plan_terminal_stack(AgentOsTerminalStackPlan* out) {
#if (defined(__linux__) || defined(__APPLE__)) && \
    (defined(__x86_64__) || defined(__aarch64__)) && \
    !defined(AGENTOS_TERMINAL_STACK_ASAN)
  if (!out) return EINVAL;
#if defined(__linux__)
  pthread_attr_t attributes;
  int result = pthread_getattr_np(pthread_self(), &attributes);
  if (result != 0) return result;
  void* stack = nullptr;
  size_t size = 0;
  size_t guard = 0;
  result = pthread_attr_getstack(&attributes, &stack, &size);
  if (result == 0) result = pthread_attr_getguardsize(&attributes, &guard);
  const int destroyed = pthread_attr_destroy(&attributes);
  if (result != 0) return result;
  if (destroyed != 0) return destroyed;
  const uintptr_t low = reinterpret_cast<uintptr_t>(stack);
  if (size > std::numeric_limits<uintptr_t>::max() - low) return EOVERFLOW;
  const uintptr_t high = low + size;
#else
  // Darwin reports the top of a downward-growing stack. libpthread computes
  // its size as stackaddr - stackbottom; the guard is outside that usable range.
  const uintptr_t high = reinterpret_cast<uintptr_t>(
      pthread_get_stackaddr_np(pthread_self()));
  const size_t size = pthread_get_stacksize_np(pthread_self());
  if (size == 0 || size > high) return ERANGE;
  const uintptr_t low = high - size;
  const size_t guard = 0;
#endif
  volatile unsigned char marker = 0;
  const uintptr_t sp = reinterpret_cast<uintptr_t>(&marker);
  // Match the pinned, non-ASan x64/arm64 V8 default budget, but explicitly own its
  // origin. Reserving below it changes recursion depth slightly from V8's own
  // deeper initialization frame; it does not increase the guest byte budget.
  constexpr uintptr_t guest_budget = 984 * 1024;
  constexpr uintptr_t terminal_reserve = 128 * 1024;
  constexpr uintptr_t host_floor_reserve = 64 * 1024;
  if (sp < low || sp >= high || guard > size ||
      size - guard < host_floor_reserve ||
      sp < guest_budget + terminal_reserve) return ERANGE;
  const uintptr_t ordinary = sp - guest_budget;
  const uintptr_t terminal = ordinary - terminal_reserve;
  // Conservatively add reported guard bytes even where pthread's usable range
  // already excludes them. Never reclaim/change an OS guard or mapped stack.
  const uintptr_t floor = low + guard + host_floor_reserve;
  if (terminal < floor) return ENOMEM;
  *out = {low, high, guard, sp, ordinary, terminal};
  return 0;
#else
  (void)out;
  return ENOTSUP;
#endif
}

extern "C" void agentos_v8_set_owned_stack_limit(v8::Isolate* isolate, uintptr_t limit) {
  isolate->SetStackLimit(limit);
}

extern "C" int agentos_v8_open_terminal_stack(
    v8::Isolate* isolate, const AgentOsTerminalStackPlan* plan) {
  volatile unsigned char marker = 0;
  const uintptr_t sp = reinterpret_cast<uintptr_t>(&marker);
  constexpr uintptr_t barrier_headroom = 32 * 1024;
  if (sp < plan->stack_low || sp >= plan->stack_high ||
      sp < plan->terminal_limit || sp - plan->terminal_limit < barrier_headroom)
    return ERANGE;
  // Public SetStackLimit preserves pending special interrupt limits. Only the
  // trusted precompiled entry runs before the caller restores ordinary_limit.
  isolate->SetStackLimit(plan->terminal_limit);
  return 0;
}

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
