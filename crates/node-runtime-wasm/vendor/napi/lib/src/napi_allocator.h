#ifndef NAPI_ALLOCATOR_H_
#define NAPI_ALLOCATOR_H_

#include <array>
#include <cassert>
#include <concepts>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <iterator>
#include <new>
#include <type_traits>

#include "napi_intrinsic_link.h"

template <typename T, typename Owner>
struct napi_allocator_lifetime__
{
  static constexpr void record_create(Owner *owner, T *val)
  {
    (void)owner;
    (void)val;
  }

  static constexpr void record_release(Owner *owner, T *val)
  {
    (void)owner;
    (void)val;
  }
};

template <typename T>
concept napi_allocator_payload__ = std::destructible<T>;

template <typename T>
concept napi_allocator_owner__ = std::is_class_v<T>;

// Blazing-fast free list implemented as linked slabs, perfect for small
// JavaScript N-API wrapper objects that fly around. Each slab stores a fixed
// number of slots, and allocation/deallocation are O(1) operations in Release
// mode.
template <napi_allocator_payload__ T, napi_allocator_owner__ Owner, size_t N = 64>
class napi_allocator__
{
public:
  static_assert(N > 0, "N must be greater than zero");

  template <bool IsConst>
  class basic_iterator__;

  using iterator = basic_iterator__<false>;
  using const_iterator = basic_iterator__<true>;

  // Release complexity: O(1).
  constexpr explicit napi_allocator__(Owner *owner) : owner_{owner} {}
  napi_allocator__(const napi_allocator__ &) = delete;
  napi_allocator__(napi_allocator__ &&other) = delete;
  napi_allocator__ &operator=(const napi_allocator__ &) = delete;
  napi_allocator__ &operator=(napi_allocator__ &&other) = delete;

  // Release complexity: O(K), where K it total number of slots allocated.
  ~napi_allocator__()
  {
    close();
  }

  // Release complexity: O(1); growing initializes one fixed-size N-slot block.
  template <typename... Args>
    requires std::constructible_from<T, Args...>
  T *allocate(Args &&...args)
  {
    napi_intrinsic_link__ *block_link = first_partial_.first();
    if (block_link == nullptr)
      block_link = first_free_.first();

    block__ *block = block_link == nullptr ? nullptr : block_link->template unsafe_get<&block__::link_>();
    if (block == nullptr)
    {
      block = new (std::nothrow) block__{owner_};
      if (block == nullptr)
        return nullptr;
      first_free_.link(block->link_);
    }

    slot__ *slot = block->allocate();
    T *data = slot->construct(static_cast<Args &&>(args)...);
    napi_allocator_lifetime__<T, Owner>::record_create(owner_, data);
    relink(block);

    return data;
  }

  // Release complexity: O(1).
  static constexpr Owner *unsafe_owner(T *data)
  {
    slot__ *slot = slot__::unsafe_slot_from_data(data);
    const block__ *block = block__::unsafe_block_from_slot(slot);

    assert(!block->is_free(slot));
    return block->owner();
  }

  // Release complexity: O(1).
  constexpr bool owns(T *data) const
  {
    return unsafe_owner(data) == owner_;
  }

  // Release complexity: O(1), excluding the payload destructor.
  void destroy(T *data)
  {
    slot__ *slot = slot__::unsafe_slot_from_data(data);
    block__ *block = block__::unsafe_block_from_slot(slot);
    const bool block_was_linked = block->link_.linked();

    assert(slot_index(block, slot) < N);
    assert(!block->is_free(slot));

    block->unlink_used_slot(slot);
    if (block_was_linked)
      block->link_.unlink();
    napi_allocator_lifetime__<T, Owner>::record_release(owner_, slot->data());
    slot->destroy();
    block->link_free_slot(slot);
    if (block_was_linked)
      link_block(block);
  }

  // Release complexity: O(1). The returned payload remains constructed.
  T *take_used()
  {
    napi_intrinsic_link__ *block_link = first_used_.first();
    block__ *block = block_link == nullptr ? nullptr : block_link->template unsafe_get<&block__::link_>();
    if (block == nullptr)
    {
      napi_intrinsic_link__ *partial_link = first_partial_.first();
      block = partial_link == nullptr ? nullptr : partial_link->template unsafe_get<&block__::link_>();
      if (block == nullptr)
        return nullptr;
    }

    slot__ *slot = block->first_used_slot();
    assert(slot != nullptr);

    T *data = slot->data();
    napi_allocator_lifetime__<T, Owner>::record_release(owner_, data);
    block->unlink_used_slot(slot);
    block->link_free_slot(slot);
    relink(block);

    return data;
  }

  // Release complexity: O(1). Alias for take_used().
  T *take_next_used()
  {
    return take_used();
  }

  // Release complexity: O(M * N), where M is allocated block count.
  void close()
  {
    while (napi_intrinsic_link__ *block_link = first_used_.first())
    {
      block__ *block = block_link->template unsafe_get<&block__::link_>();
      block->link_.unlink();
      block->close();
      delete block;
    }

    while (napi_intrinsic_link__ *block_link = first_partial_.first())
    {
      block__ *block = block_link->template unsafe_get<&block__::link_>();
      block->link_.unlink();
      block->close();
      delete block;
    }

    while (napi_intrinsic_link__ *block_link = first_free_.first())
    {
      block__ *block = block_link->template unsafe_get<&block__::link_>();
      block->link_.unlink();
      block->close();
      delete block;
    }
  }

  // Release complexity: O(M), where M is allocated block count.
  constexpr size_t slot_count() const
  {
    return storage_slot_count();
  }

  // Release complexity: O(M), where M is allocated block count.
  constexpr size_t storage_slot_count() const
  {
    return (first_used_.count() + first_partial_.count() + first_free_.count()) * N;
  }

  // Release complexity: O(M + P), where P is active slots in partial blocks;
  // worst-case O(M * N).
  constexpr size_t count_active() const
  {
    size_t count = first_used_.count() * N;
    for (const napi_intrinsic_link__ *link = first_partial_.next();
         link != &first_partial_;
         link = link->next())
    {
      count += link->template unsafe_get<&block__::link_>()->count_used();
    }
    return count;
  }

  // Release complexity: O(1). Full traversal is O(M + A), worst-case O(M * N).
  constexpr iterator begin()
  {
    return iterator{&first_used_, &first_partial_};
  }

  // Release complexity: O(1).
  constexpr iterator end()
  {
    return iterator{};
  }

  // Release complexity: O(1). Full traversal is O(M + A), worst-case O(M * N).
  constexpr const_iterator begin() const
  {
    return const_iterator{&first_used_, &first_partial_};
  }

  // Release complexity: O(1).
  constexpr const_iterator end() const
  {
    return const_iterator{};
  }

private:
  static constexpr size_t next_power_of_two__(size_t value)
  {
    size_t result = 1;

    while (result < value)
      result <<= 1;

    return result;
  }

  struct block__;

  struct slot__
  {
  public:
    alignas(T) std::byte storage_[sizeof(T)];
    napi_intrinsic_link__ free_link_{};
    napi_intrinsic_link__ used_link_{};

    constexpr T *data()
    {
      return std::launder(reinterpret_cast<T *>(storage_));
    }

    constexpr const T *data() const
    {
      return std::launder(reinterpret_cast<const T *>(storage_));
    }

    template <typename... Args>
    T *construct(Args &&...args)
    {
      return new (static_cast<void *>(storage_)) T{static_cast<Args &&>(args)...};
    }

    void destroy()
    {
      data()->~T();
#ifndef NDEBUG
      std::memset(storage_, 0, sizeof(storage_));
#endif
    }

    static constexpr slot__ *unsafe_slot_from_data(T *data)
    {
      assert(data != nullptr);
      return reinterpret_cast<slot__ *>(
          reinterpret_cast<char *>(data) - offsetof(slot__, storage_));
    }

    constexpr slot__() = default;
    slot__(const slot__ &) = delete;
    slot__(slot__ &&) = delete;
    slot__ &operator=(const slot__ &) = delete;
    slot__ &operator=(slot__ &&) = delete;
  };

  struct block_layout__
  {
    Owner *owner_ = nullptr;
    napi_intrinsic_link__ link_{};
    napi_intrinsic_link__ first_free_slot_{};
    napi_intrinsic_link__ first_used_slot_{};
    std::array<slot__, N> slots_;
  };

  static constexpr size_t block_alignment__ =
      next_power_of_two__(sizeof(block_layout__));

  static_assert((block_alignment__ & (block_alignment__ - 1)) == 0,
                "block alignment must be a power of two");

  struct alignas(block_alignment__) block__
  {
  public:
    constexpr explicit block__(Owner *owner) : owner_{owner}
    {
      reset_free_list();
    }

    block__(const block__ &) = delete;
    block__ &operator=(const block__ &) = delete;

    static constexpr block__ *unsafe_block_from_slot(slot__ *slot)
    {
      assert(slot != nullptr);
      return reinterpret_cast<block__ *>(
          reinterpret_cast<uintptr_t>(slot) & ~(static_cast<uintptr_t>(block_alignment__) - 1));
    }

    static constexpr const block__ *unsafe_block_from_slot(const slot__ *slot)
    {
      assert(slot != nullptr);
      return reinterpret_cast<const block__ *>(
          reinterpret_cast<uintptr_t>(slot) & ~(static_cast<uintptr_t>(block_alignment__) - 1));
    }

    constexpr slot__ *allocate()
    {
      napi_intrinsic_link__ *free = this->first_free_slot_.first();
      assert(free != nullptr);

      slot__ *slot = free->template unsafe_get<&slot__::free_link_>();
      slot->free_link_.unlink();
      link_used(slot);
      return slot;
    }

    constexpr void release_slot(slot__ *slot)
    {
      assert(slot != nullptr);
      assert(!is_free(slot));

      unlink_used_slot(slot);
      link_free_slot(slot);
    }

    constexpr void unlink_used_slot(slot__ *slot)
    {
      assert(slot != nullptr);
      assert(!is_free(slot));

      unlink_used(slot);
    }

    constexpr void link_free_slot(slot__ *slot)
    {
      assert(slot != nullptr);
      assert(!is_free(slot));

      this->first_free_slot_.link(slot->free_link_);
    }

    void close()
    {
      while (slot__ *slot = this->first_used_slot())
      {
        napi_allocator_lifetime__<T, Owner>::record_release(this->owner_, slot->data());
        unlink_used_slot(slot);
        slot->destroy();
        link_free_slot(slot);
      }

      reset_free_list();
    }

    constexpr Owner *owner() const
    {
      return this->owner_;
    }

    constexpr bool is_full() const
    {
      return this->first_free_slot_.first() == nullptr;
    }

    constexpr bool is_empty() const
    {
      return this->first_used_slot_.first() == nullptr;
    }

    constexpr bool is_free(const slot__ *candidate) const
    {
      assert(candidate != nullptr);
      return candidate->free_link_.linked();
    }

    constexpr slot__ *first_used_slot()
    {
      napi_intrinsic_link__ *used = this->first_used_slot_.first();
      return used == nullptr ? nullptr : used->template unsafe_get<&slot__::used_link_>();
    }

    constexpr const slot__ *first_used_slot() const
    {
      const napi_intrinsic_link__ *used = this->first_used_slot_.first();
      return used == nullptr ? nullptr : used->template unsafe_get<&slot__::used_link_>();
    }

    constexpr size_t count_used() const
    {
      return this->first_used_slot_.count();
    }

  private:
    friend class napi_allocator__<T, Owner, N>;

    constexpr void link_used(slot__ *slot)
    {
      assert(slot != nullptr);
      assert(!slot->used_link_.linked());

      this->first_used_slot_.link(slot->used_link_);
    }

    constexpr void unlink_used(slot__ *slot)
    {
      assert(slot != nullptr);

      slot->used_link_.unlink();
    }

    constexpr void reset_free_list()
    {
      while (this->first_free_slot_.first() != nullptr)
        this->first_free_slot_.first()->unlink();
      while (this->first_used_slot_.first() != nullptr)
        this->first_used_slot_.first()->unlink();

      for (size_t i = N; i > 0; --i)
      {
        slot__ &slot = this->slots_[i - 1];
        this->first_free_slot_.link(slot.free_link_);
      }
    }

    Owner *owner_ = nullptr;
    napi_intrinsic_link__ link_{};
    napi_intrinsic_link__ first_free_slot_{};
    napi_intrinsic_link__ first_used_slot_{};
    std::array<slot__, N> slots_;
  };

  static_assert(sizeof(block__) == block_alignment__,
                "napi_allocator__ block must fit exactly in its alignment region");

  static constexpr size_t slot_index(const block__ *block, const slot__ *slot)
  {
    assert(block != nullptr);
    assert(slot != nullptr);

    const slot__ *begin = block->slots_.data();
    const slot__ *end = begin + N;
    if (slot < begin || slot >= end)
      return N;

    return static_cast<size_t>(slot - begin);
  }

  constexpr void relink(block__ *block)
  {
    assert(block != nullptr);

    block->link_.unlink();
    link_block(block);
  }

  constexpr void link_block(block__ *block)
  {
    assert(block != nullptr);

    if (block->link_.linked())
      return;

    if (block->is_empty())
      first_free_.link(block->link_);
    else if (block->is_full())
      first_used_.link(block->link_);
    else
      first_partial_.link(block->link_);
  }

  napi_intrinsic_link__ first_free_{};
  napi_intrinsic_link__ first_used_{};
  napi_intrinsic_link__ first_partial_{};
  Owner *owner_ = nullptr;
};

template <napi_allocator_payload__ T, napi_allocator_owner__ Owner, size_t N>
template <bool IsConst>
class napi_allocator__<T, Owner, N>::basic_iterator__
{
public:
  using link_pointer = std::conditional_t<IsConst, const napi_intrinsic_link__ *, napi_intrinsic_link__ *>;
  using block_pointer = std::conditional_t<IsConst, const block__ *, block__ *>;
  using slot_pointer = std::conditional_t<IsConst, const slot__ *, slot__ *>;
  using iterator_category = std::forward_iterator_tag;
  using value_type = T;
  using difference_type = std::ptrdiff_t;
  using pointer = std::conditional_t<IsConst, const T *, T *>;
  using reference = std::conditional_t<IsConst, const T &, T &>;

  // Release complexity: O(1).
  constexpr basic_iterator__() = default;

  // Release complexity: O(1) amortized over full traversal.
  constexpr explicit basic_iterator__(link_pointer tail, link_pointer next_tail = nullptr)
      : tail_{tail},
        link_{tail == nullptr ? nullptr : tail->next()},
        next_tail_{next_tail}
  {
    select_used_slot();
  }

  // Release complexity: O(1).
  constexpr reference operator*() const
  {
    return *slot()->data();
  }

  // Release complexity: O(1).
  constexpr pointer operator->() const
  {
    return slot()->data();
  }

  // Release complexity: O(1) amortized over full traversal.
  constexpr basic_iterator__ &operator++()
  {
    const link_pointer used = slot_->used_link_.next();
    slot_ = used == &block()->first_used_slot_
                ? nullptr
                : used->template unsafe_get<&slot__::used_link_>();
    if (slot_ == nullptr)
    {
      link_ = link_->next();
      select_used_slot();
    }
    return *this;
  }

  // Release complexity: O(1) amortized over full traversal.
  constexpr basic_iterator__ operator++(int)
  {
    basic_iterator__ copy = *this;
    ++(*this);
    return copy;
  }

  // Release complexity: O(1).
  friend constexpr bool operator==(const basic_iterator__ &lhs, const basic_iterator__ &rhs)
  {
    return lhs.link_ == rhs.link_ &&
           (lhs.link_ == nullptr || lhs.slot_ == rhs.slot_);
  }

  // Release complexity: O(1).
  friend constexpr bool operator!=(const basic_iterator__ &lhs, const basic_iterator__ &rhs)
  {
    return !(lhs == rhs);
  }

private:
  constexpr void select_used_slot()
  {
    while (link_ != nullptr)
    {
      if (link_ == tail_)
      {
        if (next_tail_ == nullptr)
        {
          link_ = nullptr;
          return;
        }

        tail_ = next_tail_;
        link_ = tail_->next();
        next_tail_ = nullptr;
        continue;
      }

      slot_ = block()->first_used_slot();
      if (slot_ != nullptr)
        return;

      link_ = link_->next();
    }
  }

  constexpr block_pointer block() const
  {
    return link_->template unsafe_get<&block__::link_>();
  }

  constexpr slot_pointer slot() const
  {
    return slot_;
  }

  link_pointer tail_ = nullptr;
  link_pointer link_ = nullptr;
  link_pointer next_tail_ = nullptr;
  slot_pointer slot_ = nullptr;
};

#endif // NAPI_ALLOCATOR_H_
