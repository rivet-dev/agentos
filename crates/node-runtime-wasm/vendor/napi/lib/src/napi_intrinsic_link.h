#ifndef NAPI_INTRINSIC_LINK_H_
#define NAPI_INTRINSIC_LINK_H_

#include <cassert>
#include <cstddef>
#include <cstdint>

template <typename Link>
struct napi_intrinsic_link_owner__;

class napi_intrinsic_link__
{
public:
  constexpr napi_intrinsic_link__() = default;
  napi_intrinsic_link__(const napi_intrinsic_link__ &) = delete;
  napi_intrinsic_link__(napi_intrinsic_link__ &&) = delete;
  napi_intrinsic_link__ &operator=(const napi_intrinsic_link__ &) = delete;
  napi_intrinsic_link__ &operator=(napi_intrinsic_link__ &&) = delete;

  constexpr bool linked() const
  {
    return next_link_ != this;
  }

  constexpr void link(napi_intrinsic_link__ &other)
  {
    assert(!other.linked());

    other.next_link_ = next_link_;
    other.prev_link_ = this;
    next_link_->prev_link_ = &other;
    next_link_ = &other;
  }

  constexpr void unlink()
  {
    assert(linked());

    prev_link_->next_link_ = next_link_;
    next_link_->prev_link_ = prev_link_;
    next_link_ = this;
    prev_link_ = this;
  }

  constexpr napi_intrinsic_link__ *first()
  {
    return next_link_ == this ? nullptr : next_link_;
  }

  constexpr const napi_intrinsic_link__ *first() const
  {
    return next_link_ == this ? nullptr : next_link_;
  }

  constexpr bool contains(const napi_intrinsic_link__ &other) const
  {
    for (const napi_intrinsic_link__ *link = next_link_; link != this; link = link->next_link_)
    {
      if (link == &other)
        return true;
    }
    return false;
  }

  constexpr size_t count() const
  {
    size_t count = 0;
    for (const napi_intrinsic_link__ *link = next_link_; link != this; link = link->next_link_)
      ++count;
    return count;
  }

  constexpr napi_intrinsic_link__ *next()
  {
    return next_link_;
  }

  constexpr const napi_intrinsic_link__ *next() const
  {
    return next_link_;
  }

  template <auto offset>
  constexpr auto unsafe_get() -> typename napi_intrinsic_link_owner__<decltype(offset)>::type *
  {
    using T = typename napi_intrinsic_link_owner__<decltype(offset)>::type;
    const auto byte_offset = reinterpret_cast<std::uintptr_t>(&(((T *)nullptr)->*offset));
    return reinterpret_cast<T *>(reinterpret_cast<std::uintptr_t>(this) - byte_offset);
  }

  template <auto offset>
  constexpr auto unsafe_get() const -> const typename napi_intrinsic_link_owner__<decltype(offset)>::type *
  {
    using T = typename napi_intrinsic_link_owner__<decltype(offset)>::type;
    const auto byte_offset = reinterpret_cast<std::uintptr_t>(&(((T *)nullptr)->*offset));
    return reinterpret_cast<const T *>(reinterpret_cast<std::uintptr_t>(this) - byte_offset);
  }

private:
  napi_intrinsic_link__ *next_link_ = this;
  napi_intrinsic_link__ *prev_link_ = this;
};

template <typename T>
struct napi_intrinsic_link_owner__<napi_intrinsic_link__ T::*>
{
  using type = T;
};

#endif // NAPI_INTRINSIC_LINK_H_
