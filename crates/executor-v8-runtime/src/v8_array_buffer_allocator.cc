#include "v8-array-buffer.h"

#include <algorithm>
#include <memory>
#include <new>
#include <utility>

namespace {
using Allocator = v8::ArrayBuffer::Allocator;

// V8 asks for this limit before entering its allocation-failure GC path.
// The existing allocator still accounts for every live backing store.
class BoundedAllocator final : public Allocator {
 public:
  BoundedAllocator(std::unique_ptr<Allocator> inner, size_t limit)
      : inner_(std::move(inner)),
        limit_(std::min(limit, inner_->MaxAllocationSize())) {}

  size_t MaxAllocationSize() const override { return limit_; }
  void* Allocate(size_t length) override { return inner_->Allocate(length); }
  void* AllocateUninitialized(size_t length) override {
    return inner_->AllocateUninitialized(length);
  }
  void Free(void* data, size_t length) override { inner_->Free(data, length); }
  v8::PageAllocator* GetPageAllocator() override {
    return inner_->GetPageAllocator();
  }

 private:
  std::unique_ptr<Allocator> inner_;
  const size_t limit_;
};
}  // namespace

extern "C" Allocator* agentos_v8_limit_array_buffer_allocator(Allocator* inner,
                                                               size_t limit) {
  std::unique_ptr<Allocator> owner(inner);
  return new (std::nothrow) BoundedAllocator(std::move(owner), limit);
}
