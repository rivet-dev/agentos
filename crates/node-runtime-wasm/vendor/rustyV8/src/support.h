#ifndef SUPPORT_H_
#define SUPPORT_H_

#include <algorithm>
#include <array>
#include <cassert>
#include <memory>
#include <new>
#include <type_traits>
#include <utility>

#include "cppgc/name-provider.h"
#include "v8-cppgc.h"
#include "v8.h"

// Check assumptions made in binding code.
static_assert(sizeof(bool) == sizeof(uint8_t), "");
static_assert(sizeof(std::unique_ptr<void>) == sizeof(void*), "");

namespace support {
template <class T>
using uninit_t = typename std::aligned_storage<sizeof(T), alignof(T)>::type;

template <class T, class... Args>
class construct_in_place_helper {
 public:
  construct_in_place_helper(uninit_t<T>* buf, Args... args)
      : inner_(std::forward<Args>(args)...) {}

 private:
  T inner_;
};

template <class T, class... Args>
void construct_in_place(uninit_t<T>* buf, Args... args) {
  new (buf)
      construct_in_place_helper<T, Args...>(buf, std::forward<Args>(args)...);
}

// Rust's FFI only supports returning normal C data structures (AKA plain old
// data). There are some situations in the V8 API where functions return non-POD
// data. We use make_pod to carefully adjust the return values so they can be
// passed into Rust.
//
// The destructor of V is never called.
// P is not allowed to have a destructor.
template <class P>
struct make_pod {
  template <class V>
  inline make_pod(V&& value) : pod_(helper<V>(std::move(value))) {}
  template <class V>
  inline make_pod(const V& value) : pod_(helper<V>(value)) {}
  inline operator P() { return pod_; }

 private:
  P pod_;

  // This helper exists to avoid calling the destructor.
  // Using a union is a C++ trick to achieve this.
  template <class V>
  union helper {
    static_assert(std::is_trivial<P>::value &&
                      std::is_standard_layout<P>::value,
                  "type P must a pod type");
    static_assert(sizeof(V) == sizeof(P), "type P must be same size as type V");
    static_assert(alignof(V) == alignof(P),
                  "alignment of type P must be compatible with that of type V");

    inline helper(V&& value) : value_(std::move(value)) {}
    inline helper(const V& value) : value_(value) {}
    inline ~helper() {}

    inline operator P() {
      // Do a memcpy here avoid undefined behavior.
      P result;
      memcpy(&result, this, sizeof result);
      return result;
    }

   private:
    V value_;
  };
};

// The C-ABI compatible equivalent of V8's Maybe<bool>.
enum class MaybeBool { JustFalse = 0, JustTrue = 1, Nothing = 2 };

inline static MaybeBool maybe_to_maybe_bool(v8::Maybe<bool> maybe) {
  if (maybe.IsNothing()) {
    return MaybeBool::Nothing;
  } else if (maybe.FromJust()) {
    return MaybeBool::JustTrue;
  } else {
    return MaybeBool::JustFalse;
  }
}

inline static v8::Maybe<bool> maybe_bool_to_maybe(MaybeBool maybe) {
  switch (maybe) {
    case MaybeBool::JustTrue:
    case MaybeBool::JustFalse:
      return v8::Just<bool>(maybe == MaybeBool::JustTrue);
    default:
      return v8::Nothing<bool>();
  }
}

template <class T>
inline static T* local_to_ptr(v8::Local<T> local) {
  return *local;
}

template <class T>
inline static const v8::Local<T> ptr_to_local(const T* ptr) {
  static_assert(sizeof(v8::Local<T>) == sizeof(T*), "");
  auto local = *reinterpret_cast<const v8::Local<T>*>(&ptr);
  assert(*local == ptr);
  return local;
}

template <class T>
inline static v8::Local<T>* const_ptr_array_to_local_array(
    const T* const ptr_array[]) {
  static_assert(sizeof(v8::Local<T>[42]) == sizeof(T* [42]), "");
  auto mut_ptr_array = const_cast<T**>(ptr_array);
  auto mut_local_array = reinterpret_cast<v8::Local<T>*>(mut_ptr_array);
  return mut_local_array;
}

template <class T>
inline static T* maybe_local_to_ptr(v8::MaybeLocal<T> local) {
  return *local.FromMaybe(v8::Local<T>());
}

template <class T>
inline static const v8::MaybeLocal<T> ptr_to_maybe_local(const T* ptr) {
  static_assert(sizeof(v8::MaybeLocal<T>) == sizeof(T*), "");
  return *reinterpret_cast<const v8::MaybeLocal<T>*>(&ptr);
}

template <class T>
inline static v8::Global<T> ptr_to_global(const T* ptr) {
  v8::Global<T> global;
  std::swap(ptr, *reinterpret_cast<const T**>(&global));
  return global;
}

// Because, for some reason, Clang complains that `std::array<void*, 2>` is an
// incomplete type, incompatible with C linkage.
struct two_pointers_t {
  void* a;
  void* b;
};

struct three_pointers_t {
  void* a;
  void* b;
  void* c;
};

}  // namespace support

struct memory_span_t {
  uint8_t* data;
  size_t size;
};

#define EACH_TYPED_ARRAY(V) \
  V(Uint8Array)             \
  V(Uint8ClampedArray)      \
  V(Int8Array)              \
  V(Uint16Array)            \
  V(Int16Array)             \
  V(Uint32Array)            \
  V(Int32Array)             \
  V(Float32Array)           \
  V(Float64Array)           \
  V(BigUint64Array)         \
  V(BigInt64Array)

#endif  // SUPPORT_H_

class RustObj : public cppgc::GarbageCollected<RustObj>,
                public cppgc::NameProvider {
 public:
  ~RustObj();
  void Trace(cppgc::Visitor* visitor) const;
  const char* GetHumanReadableName() const final;
  uintptr_t data[2];
};
