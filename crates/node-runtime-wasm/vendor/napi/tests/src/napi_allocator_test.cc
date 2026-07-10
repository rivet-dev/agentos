#include "napi_allocator.h"

#include <algorithm>
#include <cstddef>
#include <random>
#include <vector>

#include "gtest/gtest.h"

namespace
{

struct basic_owner__
{
  int id_ = 0;
  size_t created_ = 0;
  size_t released_ = 0;
};

struct basic_payload__
{
  basic_owner__ *owner_ = nullptr;
  int id_ = 0;

  basic_payload__(basic_owner__ *owner, int id) : owner_{owner}, id_{id} {}
};

struct reentrant_payload__;

struct reentrant_owner__
{
  void *allocator_ = nullptr;
  std::vector<int> *events_ = nullptr;
  reentrant_payload__ *allocated_ = nullptr;
};

struct reentrant_payload__
{
  reentrant_owner__ *owner_ = nullptr;
  int id_ = 0;
  reentrant_payload__ *destroy_peer_ = nullptr;
  bool allocate_after_peer_ = false;

  reentrant_payload__(reentrant_owner__ *owner, int id)
      : owner_{owner}, id_{id}
  {
  }

  ~reentrant_payload__();
};

using reentrant_allocator__ = napi_allocator__<reentrant_payload__, reentrant_owner__, 4>;

} // namespace

template <>
struct napi_allocator_lifetime__<basic_payload__, basic_owner__>
{
  static void record_create(basic_owner__ *owner, basic_payload__ *payload)
  {
    (void)payload;
    ++owner->created_;
  }

  static void record_release(basic_owner__ *owner, basic_payload__ *payload)
  {
    (void)payload;
    ++owner->released_;
  }
};

namespace
{

template <size_t N>
std::vector<int> active_ids(napi_allocator__<basic_payload__, basic_owner__, N> &allocator)
{
  std::vector<int> ids;
  for (basic_payload__ &payload : allocator)
    ids.push_back(payload.id_);
  std::sort(ids.begin(), ids.end());
  return ids;
}

reentrant_payload__::~reentrant_payload__()
{
  owner_->events_->push_back(id_);
  reentrant_payload__ *peer = destroy_peer_;
  destroy_peer_ = nullptr;
  if (peer == nullptr)
    return;

  reentrant_allocator__ *allocator =
      static_cast<reentrant_allocator__ *>(owner_->allocator_);
  allocator->destroy(peer);
  if (allocate_after_peer_)
    owner_->allocated_ = allocator->allocate(owner_, id_ + 100);
}

TEST(NapiAllocator, AllocatesDestroysAndReusesSlots)
{
  basic_owner__ owner{.id_ = 1};
  napi_allocator__<basic_payload__, basic_owner__, 4> allocator{&owner};

  basic_payload__ *a = allocator.allocate(&owner, 1);
  basic_payload__ *b = allocator.allocate(&owner, 2);
  basic_payload__ *c = allocator.allocate(&owner, 3);
  basic_payload__ *d = allocator.allocate(&owner, 4);

  EXPECT_EQ(allocator.storage_slot_count(), 4u);
  EXPECT_EQ(allocator.count_active(), 4u);
  EXPECT_TRUE(allocator.owns(a));
  EXPECT_EQ((napi_allocator__<basic_payload__, basic_owner__, 4>::unsafe_owner(a)), &owner);
  EXPECT_EQ(active_ids(allocator), (std::vector<int>{1, 2, 3, 4}));

  allocator.destroy(b);
  EXPECT_EQ(allocator.count_active(), 3u);
  EXPECT_EQ(owner.released_, 1u);

  basic_payload__ *reused = allocator.allocate(&owner, 20);
  EXPECT_EQ(reused, b);
  EXPECT_EQ(allocator.count_active(), 4u);
  EXPECT_EQ(active_ids(allocator), (std::vector<int>{1, 3, 4, 20}));

  basic_payload__ *e = allocator.allocate(&owner, 5);
  EXPECT_EQ(allocator.storage_slot_count(), 8u);
  EXPECT_EQ(allocator.count_active(), 5u);

  allocator.destroy(a);
  allocator.destroy(c);
  allocator.destroy(d);
  allocator.destroy(reused);
  allocator.destroy(e);

  EXPECT_EQ(allocator.count_active(), 0u);
  EXPECT_EQ(owner.created_, 6u);
  EXPECT_EQ(owner.released_, 6u);
}

TEST(NapiAllocator, IteratesFullAndPartialBlocks)
{
  basic_owner__ owner{.id_ = 2};
  napi_allocator__<basic_payload__, basic_owner__, 3> allocator{&owner};

  std::vector<basic_payload__ *> payloads;
  for (int id = 1; id <= 5; ++id)
    payloads.push_back(allocator.allocate(&owner, id));

  EXPECT_EQ(allocator.storage_slot_count(), 6u);
  EXPECT_EQ(allocator.count_active(), 5u);
  EXPECT_EQ(active_ids(allocator), (std::vector<int>{1, 2, 3, 4, 5}));

  allocator.destroy(payloads[1]);
  allocator.destroy(payloads[3]);
  EXPECT_EQ(allocator.count_active(), 3u);
  EXPECT_EQ(active_ids(allocator), (std::vector<int>{1, 3, 5}));

  allocator.destroy(payloads[0]);
  allocator.destroy(payloads[2]);
  allocator.destroy(payloads[4]);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, AllocationPrioritizesPartialBlocksBeforeFreeBlocks)
{
  basic_owner__ owner{.id_ = 4};
  napi_allocator__<basic_payload__, basic_owner__, 2> allocator{&owner};

  basic_payload__ *a = allocator.allocate(&owner, 1);
  basic_payload__ *b = allocator.allocate(&owner, 2);
  basic_payload__ *c = allocator.allocate(&owner, 3);
  basic_payload__ *d = allocator.allocate(&owner, 4);

  allocator.destroy(a);
  allocator.destroy(c);
  allocator.destroy(d);

  basic_payload__ *reused = allocator.allocate(&owner, 10);
  EXPECT_EQ(reused, a);
  EXPECT_EQ(active_ids(allocator), (std::vector<int>{2, 10}));

  allocator.destroy(b);
  allocator.destroy(reused);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, OwnsDistinguishesAllocatorOwners)
{
  basic_owner__ first_owner{.id_ = 10};
  basic_owner__ second_owner{.id_ = 20};
  napi_allocator__<basic_payload__, basic_owner__, 2> first{&first_owner};
  napi_allocator__<basic_payload__, basic_owner__, 2> second{&second_owner};

  basic_payload__ *first_payload = first.allocate(&first_owner, 1);
  basic_payload__ *second_payload = second.allocate(&second_owner, 2);

  EXPECT_TRUE(first.owns(first_payload));
  EXPECT_FALSE(first.owns(second_payload));
  EXPECT_TRUE(second.owns(second_payload));
  EXPECT_FALSE(second.owns(first_payload));

  first.destroy(first_payload);
  second.destroy(second_payload);
}

TEST(NapiAllocator, ReusesSlotsAcrossFreePartialAndUsedTransitions)
{
  basic_owner__ owner{.id_ = 11};
  napi_allocator__<basic_payload__, basic_owner__, 2> allocator{&owner};

  basic_payload__ *a = allocator.allocate(&owner, 1);
  basic_payload__ *b = allocator.allocate(&owner, 2);
  EXPECT_EQ(allocator.count_active(), 2u);

  allocator.destroy(a);
  EXPECT_EQ(allocator.count_active(), 1u);
  basic_payload__ *partial_reuse = allocator.allocate(&owner, 3);
  EXPECT_EQ(partial_reuse, a);

  allocator.destroy(b);
  allocator.destroy(partial_reuse);
  EXPECT_EQ(allocator.count_active(), 0u);
  basic_payload__ *free_reuse = allocator.allocate(&owner, 4);
  EXPECT_TRUE(free_reuse == a || free_reuse == b);

  allocator.destroy(free_reuse);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, TakeUsedReturnsConstructedPayloadAndReleasesSlot)
{
  basic_owner__ owner{.id_ = 3};
  napi_allocator__<basic_payload__, basic_owner__, 2> allocator{&owner};

  basic_payload__ *a = allocator.allocate(&owner, 1);
  basic_payload__ *b = allocator.allocate(&owner, 2);
  basic_payload__ *c = allocator.allocate(&owner, 3);

  basic_payload__ *taken = allocator.take_used();
  ASSERT_NE(taken, nullptr);
  EXPECT_TRUE(taken == a || taken == b);
  EXPECT_EQ(allocator.count_active(), 2u);
  EXPECT_EQ(owner.released_, 1u);
  taken->~basic_payload__();

  basic_payload__ *taken_partial = allocator.take_used();
  ASSERT_NE(taken_partial, nullptr);
  EXPECT_EQ(allocator.count_active(), 1u);
  EXPECT_EQ(owner.released_, 2u);
  taken_partial->~basic_payload__();

  if (a != taken && a != taken_partial)
    allocator.destroy(a);
  if (b != taken && b != taken_partial)
    allocator.destroy(b);
  if (c != taken && c != taken_partial)
    allocator.destroy(c);

  EXPECT_EQ(allocator.count_active(), 0u);
  EXPECT_EQ(owner.created_, 3u);
  EXPECT_EQ(owner.released_, 3u);
}

TEST(NapiAllocator, TakeUsedDrainsFullAndPartialBlocks)
{
  basic_owner__ owner{.id_ = 12};
  napi_allocator__<basic_payload__, basic_owner__, 3> allocator{&owner};

  std::vector<basic_payload__ *> payloads;
  for (int id = 1; id <= 7; ++id)
    payloads.push_back(allocator.allocate(&owner, id));

  std::vector<int> taken_ids;
  while (basic_payload__ *payload = allocator.take_used())
  {
    taken_ids.push_back(payload->id_);
    payload->~basic_payload__();
  }

  std::sort(taken_ids.begin(), taken_ids.end());
  EXPECT_EQ(taken_ids, (std::vector<int>{1, 2, 3, 4, 5, 6, 7}));
  EXPECT_EQ(allocator.count_active(), 0u);
  EXPECT_EQ(owner.created_, 7u);
  EXPECT_EQ(owner.released_, 7u);
}

TEST(NapiAllocator, DestroySupportsReentrantSiblingDestroyInFullBlock)
{
  std::vector<int> events;
  reentrant_owner__ owner{.events_ = &events};
  reentrant_allocator__ allocator{&owner};
  owner.allocator_ = &allocator;

  reentrant_payload__ *a = allocator.allocate(&owner, 1);
  reentrant_payload__ *b = allocator.allocate(&owner, 2);
  reentrant_payload__ *c = allocator.allocate(&owner, 3);
  reentrant_payload__ *d = allocator.allocate(&owner, 4);
  a->destroy_peer_ = b;

  allocator.destroy(a);

  EXPECT_EQ((std::vector<int>{1, 2}), events);
  EXPECT_EQ(allocator.count_active(), 2u);

  allocator.destroy(c);
  allocator.destroy(d);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, NestedDestroyKeepsVacuumBlockUnavailableToAllocation)
{
  std::vector<int> events;
  reentrant_owner__ owner{.events_ = &events};
  reentrant_allocator__ allocator{&owner};
  owner.allocator_ = &allocator;

  reentrant_payload__ *a = allocator.allocate(&owner, 1);
  reentrant_payload__ *b = allocator.allocate(&owner, 2);
  reentrant_payload__ *c = allocator.allocate(&owner, 3);
  reentrant_payload__ *d = allocator.allocate(&owner, 4);
  a->destroy_peer_ = b;
  a->allocate_after_peer_ = true;

  allocator.destroy(a);

  ASSERT_NE(owner.allocated_, nullptr);
  EXPECT_NE(owner.allocated_, a);
  EXPECT_NE(owner.allocated_, b);
  EXPECT_NE(owner.allocated_, c);
  EXPECT_NE(owner.allocated_, d);
  EXPECT_EQ((std::vector<int>{1, 2}), events);
  EXPECT_EQ(allocator.count_active(), 3u);

  allocator.destroy(c);
  allocator.destroy(d);
  allocator.destroy(owner.allocated_);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, CascadingNestedDestroyKeepsPartialBlockVacuumUnavailable)
{
  std::vector<int> events;
  reentrant_owner__ owner{.events_ = &events};
  reentrant_allocator__ allocator{&owner};
  owner.allocator_ = &allocator;

  reentrant_payload__ *a = allocator.allocate(&owner, 1);
  reentrant_payload__ *b = allocator.allocate(&owner, 2);
  reentrant_payload__ *c = allocator.allocate(&owner, 3);
  a->destroy_peer_ = b;
  b->destroy_peer_ = c;
  a->allocate_after_peer_ = true;

  allocator.destroy(a);

  ASSERT_NE(owner.allocated_, nullptr);
  EXPECT_NE(owner.allocated_, a);
  EXPECT_NE(owner.allocated_, b);
  EXPECT_NE(owner.allocated_, c);
  EXPECT_EQ((std::vector<int>{1, 2, 3}), events);
  EXPECT_EQ(allocator.count_active(), 1u);

  allocator.destroy(owner.allocated_);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, NestedDestroyInDifferentBlockCanBeReusedImmediately)
{
  std::vector<int> events;
  reentrant_owner__ owner{.events_ = &events};
  reentrant_allocator__ allocator{&owner};
  owner.allocator_ = &allocator;

  reentrant_payload__ *a = allocator.allocate(&owner, 1);
  reentrant_payload__ *b = allocator.allocate(&owner, 2);
  reentrant_payload__ *c = allocator.allocate(&owner, 3);
  reentrant_payload__ *d = allocator.allocate(&owner, 4);
  reentrant_payload__ *e = allocator.allocate(&owner, 5);
  a->destroy_peer_ = e;
  a->allocate_after_peer_ = true;

  allocator.destroy(a);

  EXPECT_EQ(owner.allocated_, e);
  EXPECT_EQ((std::vector<int>{1, 5}), events);
  EXPECT_EQ(allocator.count_active(), 4u);

  allocator.destroy(b);
  allocator.destroy(c);
  allocator.destroy(d);
  allocator.destroy(owner.allocated_);
  EXPECT_EQ(allocator.count_active(), 0u);
}

TEST(NapiAllocator, CloseSupportsReentrantSiblingDestroyInFullBlock)
{
  std::vector<int> events;
  reentrant_owner__ owner{.events_ = &events};
  reentrant_allocator__ allocator{&owner};
  owner.allocator_ = &allocator;

  reentrant_payload__ *a = allocator.allocate(&owner, 1);
  reentrant_payload__ *b = allocator.allocate(&owner, 2);
  reentrant_payload__ *c = allocator.allocate(&owner, 3);
  reentrant_payload__ *d = allocator.allocate(&owner, 4);
  (void)a;
  (void)b;
  d->destroy_peer_ = c;

  allocator.close();

  EXPECT_EQ(allocator.count_active(), 0u);
  EXPECT_EQ(allocator.storage_slot_count(), 0u);
  EXPECT_EQ(events.size(), 4u);
  EXPECT_NE(std::find(events.begin(), events.end(), 1), events.end());
  EXPECT_NE(std::find(events.begin(), events.end(), 2), events.end());
  EXPECT_NE(std::find(events.begin(), events.end(), 3), events.end());
  EXPECT_NE(std::find(events.begin(), events.end(), 4), events.end());
}

TEST(NapiAllocator, DeterministicRandomOperationsMatchPointerModel)
{
  basic_owner__ owner{.id_ = 13};
  napi_allocator__<basic_payload__, basic_owner__, 5> allocator{&owner};
  std::vector<basic_payload__ *> active;
  std::mt19937 rng{0x5A17BEEF};
  int next_id = 1;

  for (size_t step = 0; step < 1000; ++step)
  {
    const unsigned choice = rng() % 100;
    if (active.empty() || choice < 50)
    {
      active.push_back(allocator.allocate(&owner, next_id++));
    }
    else if (choice < 85)
    {
      const size_t index = rng() % active.size();
      allocator.destroy(active[index]);
      active[index] = active.back();
      active.pop_back();
    }
    else
    {
      basic_payload__ *taken = allocator.take_used();
      ASSERT_NE(taken, nullptr);
      const auto found = std::find(active.begin(), active.end(), taken);
      ASSERT_NE(found, active.end());
      active.erase(found);
      taken->~basic_payload__();
    }

    ASSERT_EQ(allocator.count_active(), active.size());
    ASSERT_EQ(allocator.storage_slot_count() % 5, 0u);
    ASSERT_GE(allocator.storage_slot_count(), active.size());

    if (step % 17 == 0)
    {
      std::vector<int> expected;
      expected.reserve(active.size());
      for (basic_payload__ *payload : active)
      {
        ASSERT_TRUE(allocator.owns(payload));
        expected.push_back(payload->id_);
      }
      std::sort(expected.begin(), expected.end());
      EXPECT_EQ(active_ids(allocator), expected);
    }
  }

  for (basic_payload__ *payload : active)
    allocator.destroy(payload);

  EXPECT_EQ(allocator.count_active(), 0u);
  EXPECT_EQ(owner.created_, owner.released_);
}

} // namespace
