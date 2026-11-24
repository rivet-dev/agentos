// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
#include <algorithm>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <map>
#include <memory>
#include <mutex>
#include <span>
#include <string_view>
#include <thread>
#include <vector>

#include "cppgc/allocation.h"
#include "cppgc/persistent.h"
#include "cppgc/platform.h"
#include "libplatform/libplatform.h"
#include "support.h"
#include "unicode/locid.h"
#include "unicode/timezone.h"
#include "unicode/unistr.h"
#include "v8-callbacks.h"
#include "v8-cppgc.h"
#include "v8-fast-api-calls.h"
#include "v8-inspector-protocol.h"
#include "v8-inspector.h"
#include "v8-internal.h"
#include "v8-platform.h"
#include "v8-profiler.h"
#include "v8.h"
#include "v8/src/execution/isolate-inl.h"
#include "v8/src/execution/isolate.h"
#include "v8/src/execution/microtask-queue.h"
#include "v8/src/flags/flags.h"
#include "v8/src/heap/base-page.h"
#include "v8/src/heap/memory-chunk-inl.h"
#include "v8/src/libplatform/default-platform.h"

using namespace support;

// TODO(bartlomieju): ideally we could ignore only some of the deprecated APIs
#pragma clang diagnostic ignored "-Wdeprecated-declarations"

template <typename T>
constexpr size_t align_to(size_t size) {
  return (size + sizeof(T) - 1) & ~(sizeof(T) - 1);
}

#ifdef V8_ENABLE_CHECKS
constexpr size_t handle_scope_size = 4;
#else
constexpr size_t handle_scope_size = 3;
#endif

static_assert(sizeof(two_pointers_t) ==
                  sizeof(std::shared_ptr<v8::BackingStore>),
              "std::shared_ptr<v8::BackingStore> size mismatch");

static_assert(sizeof(v8::HandleScope) == sizeof(size_t) * handle_scope_size,
              "HandleScope size mismatch");

static_assert(sizeof(v8::EscapableHandleScope) ==
                  sizeof(size_t) * (handle_scope_size + 1),
              "EscapableHandleScope size mismatch");

static_assert(sizeof(v8::PromiseRejectMessage) == sizeof(size_t) * 3,
              "PromiseRejectMessage size mismatch");

static_assert(sizeof(v8::IsolateGroup) == sizeof(size_t),
              "IsolateGroup size mismatch");

static_assert(sizeof(v8::Locker) == sizeof(size_t) * 2, "Locker size mismatch");

static_assert(sizeof(v8::Unlocker) == sizeof(size_t) * 1,
              "Unlocker size mismatch");

static_assert(sizeof(v8::ScriptCompiler::CompilationDetails) ==
                  sizeof(int64_t) * 3,
              "CompilationDetails size mismatch");

static_assert(
    sizeof(v8::ScriptCompiler::Source) ==
        align_to<size_t>(sizeof(size_t) * 8 + sizeof(int) * 2 +
                         // the last field before CompilationDetails on 32-bit
                         // systems will have a padding
                         align_to<int64_t>(sizeof(size_t)) +
                         sizeof(v8::ScriptCompiler::CompilationDetails)),
    "Source size mismatch");

static_assert(sizeof(v8::ReturnValue<v8::Value>) == sizeof(size_t) * 1,
              "ReturnValue size mismatch");

static_assert(sizeof(v8::TryCatch) == sizeof(size_t) * 6,
              "TryCatch size mismatch");

static_assert(sizeof(v8::Isolate::AllowJavascriptExecutionScope) ==
                  sizeof(size_t) * 2,
              "AllowJavascriptExecutionScope size mismatch");

static_assert(sizeof(v8::Location) == sizeof(int) * 2,
              "Location size mismatch");

static_assert(sizeof(v8::SnapshotCreator) == sizeof(size_t) * 1,
              "SnapshotCreator size mismatch");

static_assert(sizeof(v8::CFunction) == sizeof(size_t) * 2,
              "CFunction size mismatch");

static_assert(sizeof(three_pointers_t) == sizeof(v8_inspector::StringView),
              "StringView size mismatch");

#if INTPTR_MAX == INT64_MAX  // 64-bit platforms
static_assert(sizeof(v8::ScriptCompiler::CachedData) == 24,
              "CachedData size mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, data) == 0,
              "CachedData.data offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, length) == 8,
              "CachedData.length offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, rejected) == 12,
              "CachedData.rejected offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, buffer_policy) == 16,
              "CachedData.buffer_policy offset mismatch");
static_assert(sizeof(v8::Isolate::DisallowJavascriptExecutionScope) == 16,
              "DisallowJavascriptExecutionScope size mismatch");
#else
static_assert(sizeof(v8::ScriptCompiler::CachedData) == 16,
              "CachedData size mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, data) == 0,
              "CachedData.data offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, length) == 4,
              "CachedData.length offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, rejected) == 8,
              "CachedData.rejected offset mismatch");
static_assert(offsetof(v8::ScriptCompiler::CachedData, buffer_policy) == 12,
              "CachedData.buffer_policy offset mismatch");
static_assert(sizeof(v8::Isolate::DisallowJavascriptExecutionScope) == 12,
              "DisallowJavascriptExecutionScope size mismatch");
#endif

// Note: this currently uses an internal API to determine if the v8 sandbox is
// enabled in the testsuite etc.
extern "C" bool v8__V8__IsSandboxEnabled() {
  return v8::internal::SandboxIsEnabled();
}

namespace {

// Returns the isolate whose heap holds `value`, or nullptr if `value` is a Smi
// or lives in read-only or writable shared space. Those spaces belong to an
// isolate group rather than to an isolate.
const v8::internal::Isolate* MaybeGetIsolate(v8::internal::Address value) {
  namespace i = v8::internal;
  if (!i::Internals::HasHeapObjectTag(value)) return nullptr;
  const i::MemoryChunk* chunk = i::MemoryChunk::FromAddress(value);
  if (chunk->InReadOnlySpace() || chunk->InWritableSharedSpace()) {
    return nullptr;
  }
  return i::Isolate::FromHeap(chunk->MetadataNoIsolateCheck()->heap());
}

// Returns whether two heap objects belong to the same isolate group. Each
// group has its own pointer compression cage when there can be more than one
// group; otherwise all isolates are in the same group.
bool InSameIsolateGroup(v8::internal::Address a, v8::internal::Address b) {
#ifdef V8_COMPRESS_POINTERS_IN_MULTIPLE_CAGES
  namespace i = v8::internal;
  constexpr i::Address kCageMask = ~(i::kPtrComprCageBaseAlignment - 1);
  return (a & kCageMask) == (b & kCageMask);
#else
  return true;
#endif
}

}  // namespace

extern "C" {

const v8::Isolate* v8__Data__MaybeGetIsolate(const v8::Data& self) {
  namespace i = v8::internal;
  return reinterpret_cast<const v8::Isolate*>(
      MaybeGetIsolate(i::ValueHelper::ValueAsAddress(&self)));
}

// Smis can be used in any isolate, read-only and writable shared space values
// in any isolate of the group they belong to, and all other values only in the
// isolate whose heap holds them.
bool v8__Data__IsCompatibleWithIsolate(const v8::Data& self,
                                       const v8::Isolate& isolate) {
  namespace i = v8::internal;
  i::Address value = i::ValueHelper::ValueAsAddress(&self);
  if (!i::Internals::HasHeapObjectTag(value)) return true;
  const i::Isolate* i_isolate = reinterpret_cast<const i::Isolate*>(&isolate);
  if (!InSameIsolateGroup(value, i_isolate->cage_base())) return false;
  const i::Isolate* owner = MaybeGetIsolate(value);
  return owner == nullptr || owner == i_isolate;
}

// Returns whether `self` can be used in an isolate that `other` can be used
// in. When neither value identifies an isolate, only their groups are
// compared.
bool v8__Data__IsCompatibleWith(const v8::Data& self, const v8::Data& other) {
  namespace i = v8::internal;
  i::Address a = i::ValueHelper::ValueAsAddress(&self);
  i::Address b = i::ValueHelper::ValueAsAddress(&other);
  if (!i::Internals::HasHeapObjectTag(a) ||
      !i::Internals::HasHeapObjectTag(b)) {
    return true;
  }
  if (!InSameIsolateGroup(a, b)) return false;
  const i::Isolate* owner_a = MaybeGetIsolate(a);
  const i::Isolate* owner_b = MaybeGetIsolate(b);
  return owner_a == nullptr || owner_b == nullptr || owner_a == owner_b;
}

void v8__V8__SetFlagsFromCommandLine(int* argc, char** argv,
                                     const char* usage) {
  namespace i = v8::internal;
  using HelpOptions = i::FlagList::HelpOptions;
  HelpOptions help_options = HelpOptions(HelpOptions::kExit, usage);
  i::FlagList::SetFlagsFromCommandLine(argc, argv, true, help_options);
}

void v8__V8__SetFlagsFromString(const char* flags, size_t length) {
  v8::V8::SetFlagsFromString(flags, length);
}

void v8__V8__SetEntropySource(v8::EntropySource callback) {
  v8::V8::SetEntropySource(callback);
}

const char* v8__V8__GetVersion() { return v8::V8::GetVersion(); }

void v8__V8__InitializePlatform(v8::Platform* platform) {
  v8::V8::InitializePlatform(platform);
}

void v8__V8__Initialize() { v8::V8::Initialize(); }

bool v8__V8__Dispose() { return v8::V8::Dispose(); }

void v8__V8__DisposePlatform() { v8::V8::DisposePlatform(); }

void v8__V8__SetFatalErrorHandler(v8::V8FatalErrorCallback that) {
  v8::V8::SetFatalErrorHandler(that);
}

bool v8__V8__EnableWebAssemblyTrapHandler(bool use_v8_signal_handler) {
  return v8::V8::EnableWebAssemblyTrapHandler(use_v8_signal_handler);
}

v8::Isolate* v8__Isolate__New(const v8::Isolate::CreateParams& params) {
  return v8::Isolate::New(params);
}

v8::Isolate* v8__Isolate__NewInGroup(const v8::IsolateGroup& group,
                                     const v8::Isolate::CreateParams& params) {
  return v8::Isolate::New(group, params);
}

// `v8::IsolateGroup` is a reference-counted handle whose only member is an
// `internal::IsolateGroup*`. `make_pod` moves that pointer out without running
// the destructor, so the reference acquired by `GetDefault`/`Create`/the copy
// constructor is handed to Rust rather than released here.
v8::internal::IsolateGroup* v8__IsolateGroup__GetDefault() {
  return make_pod<v8::internal::IsolateGroup*>(v8::IsolateGroup::GetDefault());
}

bool v8__IsolateGroup__CanCreateNewGroups() {
  return v8::IsolateGroup::CanCreateNewGroups();
}

v8::internal::IsolateGroup* v8__IsolateGroup__Create() {
  return make_pod<v8::internal::IsolateGroup*>(v8::IsolateGroup::Create());
}

v8::internal::IsolateGroup* v8__IsolateGroup__CLONE(
    const v8::IsolateGroup& self) {
  return make_pod<v8::internal::IsolateGroup*>(v8::IsolateGroup(self));
}

v8::internal::IsolateGroup* v8__Isolate__GetGroup(const v8::Isolate* isolate) {
  return make_pod<v8::internal::IsolateGroup*>(isolate->GetGroup());
}

void v8__IsolateGroup__DESTRUCT(v8::IsolateGroup* self) {
  self->~IsolateGroup();
}

bool v8__IsolateGroup__EQ(const v8::IsolateGroup& self,
                          const v8::IsolateGroup& other) {
  return self == other;
}

void v8__Isolate__Dispose(v8::Isolate* isolate) { isolate->Dispose(); }

void v8__Isolate__Enter(v8::Isolate* isolate) { isolate->Enter(); }

void v8__Locker__CONSTRUCT(uninit_t<v8::Locker>* buf, v8::Isolate* isolate) {
  construct_in_place<v8::Locker>(buf, isolate);
}

void v8__Locker__DESTRUCT(v8::Locker* self) { self->~Locker(); }

void v8__Unlocker__CONSTRUCT(uninit_t<v8::Unlocker>* buf,
                             v8::Isolate* isolate) {
  construct_in_place<v8::Unlocker>(buf, isolate);
}

void v8__Unlocker__DESTRUCT(v8::Unlocker* self) { self->~Unlocker(); }

bool v8__Locker__IsLocked(v8::Isolate* isolate) {
  return v8::Locker::IsLocked(isolate);
}

void v8__Isolate__Exit(v8::Isolate* isolate) { isolate->Exit(); }

v8::Isolate* v8__Isolate__GetCurrent() { return v8::Isolate::GetCurrent(); }

v8::Isolate* v8__Isolate__TryGetCurrent() {
  return v8::Isolate::TryGetCurrent();
}

const v8::Data* v8__Isolate__GetCurrentHostDefinedOptions(
    v8::Isolate* isolate) {
  return maybe_local_to_ptr(isolate->GetCurrentHostDefinedOptions());
}

void v8__Isolate__MemoryPressureNotification(v8::Isolate* isolate,
                                             v8::MemoryPressureLevel level) {
  isolate->MemoryPressureNotification(level);
}

void v8__Isolate__ClearKeptObjects(v8::Isolate* isolate) {
  isolate->ClearKeptObjects();
}

void v8__Isolate__LowMemoryNotification(v8::Isolate* isolate) {
  isolate->LowMemoryNotification();
}

void v8__Isolate__ContextDisposedNotification(
    v8::Isolate* isolate, v8::ContextDependants dependants) {
  isolate->ContextDisposedNotification(dependants);
}

void v8__Isolate__SetIdle(v8::Isolate* isolate, bool is_idle) {
  isolate->SetIdle(is_idle);
}

void v8__Isolate__GetHeapStatistics(v8::Isolate* isolate,
                                    v8::HeapStatistics* s) {
  isolate->GetHeapStatistics(s);
}

const v8::Context* v8__Isolate__GetCurrentContext(v8::Isolate* isolate) {
  return local_to_ptr(isolate->GetCurrentContext());
}

const v8::Context* v8__Isolate__GetEnteredOrMicrotaskContext(
    v8::Isolate* isolate) {
  return local_to_ptr(isolate->GetEnteredOrMicrotaskContext());
}

uint32_t v8__Isolate__GetNumberOfDataSlots(v8::Isolate* isolate) {
  return isolate->GetNumberOfDataSlots();
}

void* v8__Isolate__GetData(v8::Isolate* isolate, uint32_t slot) {
  return isolate->GetData(slot);
}

void v8__Isolate__SetData(v8::Isolate* isolate, uint32_t slot, void* data) {
  isolate->SetData(slot, data);
}

const v8::Data* v8__Isolate__GetDataFromSnapshotOnce(v8::Isolate* isolate,
                                                     size_t index) {
  return maybe_local_to_ptr(isolate->GetDataFromSnapshotOnce<v8::Data>(index));
}

v8::MicrotasksPolicy v8__Isolate__GetMicrotasksPolicy(
    const v8::Isolate* isolate) {
  return isolate->GetMicrotasksPolicy();
}

void v8__Isolate__SetMicrotasksPolicy(v8::Isolate* isolate,
                                      v8::MicrotasksPolicy policy) {
  static_assert(0 == static_cast<uint32_t>(v8::MicrotasksPolicy::kExplicit),
                "v8::MicrotasksPolicy::kExplicit mismatch");
  static_assert(1 == static_cast<uint32_t>(v8::MicrotasksPolicy::kScoped),
                "v8::MicrotasksPolicy::kScoped mismatch");
  static_assert(2 == static_cast<uint32_t>(v8::MicrotasksPolicy::kAuto),
                "v8::MicrotasksPolicy::kAuto mismatch");
  isolate->SetMicrotasksPolicy(policy);
}

void v8__Isolate__PerformMicrotaskCheckpoint(v8::Isolate* isolate) {
  isolate->PerformMicrotaskCheckpoint();
}

void v8__Isolate__EnqueueMicrotask(v8::Isolate* isolate,
                                   const v8::Function& function) {
  isolate->EnqueueMicrotask(ptr_to_local(&function));
}

void v8__Isolate__RequestInterrupt(v8::Isolate* isolate,
                                   v8::InterruptCallback callback, void* data) {
  isolate->RequestInterrupt(callback, data);
}

void v8__Isolate__SetPrepareStackTraceCallback(
    v8::Isolate* isolate, v8::PrepareStackTraceCallback callback) {
  isolate->SetPrepareStackTraceCallback(callback);
}

void v8__Isolate__SetPromiseHook(v8::Isolate* isolate, v8::PromiseHook hook) {
  isolate->SetPromiseHook(hook);
}

void v8__Isolate__SetPromiseRejectCallback(v8::Isolate* isolate,
                                           v8::PromiseRejectCallback callback) {
  isolate->SetPromiseRejectCallback(callback);
}

void v8__Isolate__SetWasmAsyncResolvePromiseCallback(
    v8::Isolate* isolate, v8::WasmAsyncResolvePromiseCallback callback) {
  isolate->SetWasmAsyncResolvePromiseCallback(callback);
}

void v8__Isolate__SetCaptureStackTraceForUncaughtExceptions(
    v8::Isolate* isolate, bool capture, int frame_limit) {
  isolate->SetCaptureStackTraceForUncaughtExceptions(capture, frame_limit);
}

void v8__Isolate__SetHostInitializeImportMetaObjectCallback(
    v8::Isolate* isolate, v8::HostInitializeImportMetaObjectCallback callback) {
  isolate->SetHostInitializeImportMetaObjectCallback(callback);
}

void v8__Isolate__SetHostImportModuleDynamicallyCallback(
    v8::Isolate* isolate, v8::HostImportModuleDynamicallyCallback callback) {
  isolate->SetHostImportModuleDynamicallyCallback(callback);
}

void v8__Isolate__SetHostImportModuleWithPhaseDynamicallyCallback(
    v8::Isolate* isolate,
    v8::HostImportModuleWithPhaseDynamicallyCallback callback) {
  isolate->SetHostImportModuleWithPhaseDynamicallyCallback(callback);
}

void v8__Isolate__SetHostCreateShadowRealmContextCallback(
    v8::Isolate* isolate, v8::HostCreateShadowRealmContextCallback callback) {
  isolate->SetHostCreateShadowRealmContextCallback(callback);
}

void v8__Isolate__SetUseCounterCallback(
    v8::Isolate* isolate, v8::Isolate::UseCounterCallback callback) {
  isolate->SetUseCounterCallback(callback);
}

void v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
    v8::Isolate* isolate,
    v8::ModifyCodeGenerationFromStringsCallback2 callback) {
  isolate->SetModifyCodeGenerationFromStringsCallback(callback);
}

bool v8__Isolate__AddMessageListener(v8::Isolate* isolate,
                                     v8::MessageCallback callback) {
  return isolate->AddMessageListener(callback);
}

bool v8__Isolate__AddMessageListenerWithErrorLevel(v8::Isolate* isolate,
                                                   v8::MessageCallback callback,
                                                   int error_level) {
  return isolate->AddMessageListenerWithErrorLevel(callback, error_level);
}

void v8__Isolate__AddGCPrologueCallback(
    v8::Isolate* isolate, v8::Isolate::GCCallbackWithData callback, void* data,
    v8::GCType gc_type_filter) {
  isolate->AddGCPrologueCallback(callback, data, gc_type_filter);
}

void v8__Isolate__RemoveGCPrologueCallback(
    v8::Isolate* isolate, v8::Isolate::GCCallbackWithData callback,
    void* data) {
  isolate->RemoveGCPrologueCallback(callback, data);
}

void v8__Isolate__AddGCEpilogueCallback(
    v8::Isolate* isolate, v8::Isolate::GCCallbackWithData callback, void* data,
    v8::GCType gc_type_filter) {
  isolate->AddGCEpilogueCallback(callback, data, gc_type_filter);
}

void v8__Isolate__RemoveGCEpilogueCallback(
    v8::Isolate* isolate, v8::Isolate::GCCallbackWithData callback,
    void* data) {
  isolate->RemoveGCEpilogueCallback(callback, data);
}

void v8__Isolate__AddNearHeapLimitCallback(v8::Isolate* isolate,
                                           v8::NearHeapLimitCallback callback,
                                           void* data) {
  isolate->AddNearHeapLimitCallback(callback, data);
}

size_t v8__Isolate__NumberOfHeapSpaces(v8::Isolate* isolate) {
  return isolate->NumberOfHeapSpaces();
}

bool v8__Isolate__GetHeapSpaceStatistics(
    v8::Isolate* isolate, v8::HeapSpaceStatistics* space_statistics,
    size_t index) {
  return isolate->GetHeapSpaceStatistics(space_statistics, index);
}

bool v8__Isolate__GetHeapCodeAndMetadataStatistics(v8::Isolate* isolate,
                                                   v8::HeapCodeStatistics* s) {
  return isolate->GetHeapCodeAndMetadataStatistics(s);
}

void v8__Isolate__RemoveNearHeapLimitCallback(
    v8::Isolate* isolate, v8::NearHeapLimitCallback callback,
    size_t heap_limit) {
  isolate->RemoveNearHeapLimitCallback(callback, heap_limit);
}

int64_t v8__Isolate__AdjustAmountOfExternalAllocatedMemory(
    v8::Isolate* isolate, int64_t change_in_bytes) {
  return isolate->AdjustAmountOfExternalAllocatedMemory(change_in_bytes);
}

void v8__Isolate__SetOOMErrorHandler(v8::Isolate* isolate,
                                     v8::OOMErrorCallback callback) {
  isolate->SetOOMErrorHandler(callback);
}

const v8::Value* v8__Isolate__ThrowException(v8::Isolate* isolate,
                                             const v8::Value& exception) {
  return local_to_ptr(isolate->ThrowException(ptr_to_local(&exception)));
}

void v8__Isolate__TerminateExecution(v8::Isolate* isolate) {
  isolate->TerminateExecution();
}

bool v8__Isolate__IsExecutionTerminating(v8::Isolate* isolate) {
  return isolate->IsExecutionTerminating();
}

void v8__Isolate__CancelTerminateExecution(v8::Isolate* isolate) {
  isolate->CancelTerminateExecution();
}

void v8__Isolate__SetAllowAtomicsWait(v8::Isolate* isolate, bool allow) {
  isolate->SetAllowAtomicsWait(allow);
}

void v8__Isolate__SetWasmStreamingCallback(v8::Isolate* isolate,
                                           v8::WasmStreamingCallback callback) {
  isolate->SetWasmStreamingCallback(callback);
}

void v8__Isolate__SetAllowWasmCodeGenerationCallback(
    v8::Isolate* isolate, v8::AllowWasmCodeGenerationCallback callback) {
  isolate->SetAllowWasmCodeGenerationCallback(callback);
}

bool v8__Isolate__HasPendingBackgroundTasks(v8::Isolate* isolate) {
  return isolate->HasPendingBackgroundTasks();
}

void v8__Isolate__RequestGarbageCollectionForTesting(
    v8::Isolate* isolate, v8::Isolate::GarbageCollectionType type) {
  isolate->RequestGarbageCollectionForTesting(type);
}

void v8__Isolate__CreateParams__CONSTRUCT(
    uninit_t<v8::Isolate::CreateParams>* buf) {
  construct_in_place<v8::Isolate::CreateParams>(buf);
}

size_t v8__Isolate__CreateParams__SIZEOF() {
  return sizeof(v8::Isolate::CreateParams);
}

void v8__Isolate__DateTimeConfigurationChangeNotification(
    v8::Isolate* isolate, v8::Isolate::TimeZoneDetection time_zone_detection) {
  isolate->DateTimeConfigurationChangeNotification(time_zone_detection);
}

void v8__ResourceConstraints__ConfigureDefaultsFromHeapSize(
    v8::ResourceConstraints* constraints, size_t initial_heap_size_in_bytes,
    size_t maximum_heap_size_in_bytes) {
  constraints->ConfigureDefaultsFromHeapSize(initial_heap_size_in_bytes,
                                             maximum_heap_size_in_bytes);
}

void v8__ResourceConstraints__ConfigureDefaults(
    v8::ResourceConstraints* constraints, uint64_t physical_memory,
    uint64_t virtual_memory_limit) {
  constraints->ConfigureDefaults(physical_memory, virtual_memory_limit);
}

size_t v8__ResourceConstraints__max_old_generation_size_in_bytes(
    const v8::ResourceConstraints* constraints) {
  return constraints->max_old_generation_size_in_bytes();
}

void v8__ResourceConstraints__set_max_old_generation_size_in_bytes(
    v8::ResourceConstraints* constraints, size_t limit) {
  constraints->set_max_old_generation_size_in_bytes(limit);
}

size_t v8__ResourceConstraints__max_young_generation_size_in_bytes(
    const v8::ResourceConstraints* constraints) {
  return constraints->max_young_generation_size_in_bytes();
}

void v8__ResourceConstraints__set_max_young_generation_size_in_bytes(
    v8::ResourceConstraints* constraints, size_t limit) {
  constraints->set_max_young_generation_size_in_bytes(limit);
}

size_t v8__ResourceConstraints__code_range_size_in_bytes(
    const v8::ResourceConstraints* constraints) {
  return constraints->code_range_size_in_bytes();
}

void v8__ResourceConstraints__set_code_range_size_in_bytes(
    v8::ResourceConstraints* constraints, size_t limit) {
  constraints->set_code_range_size_in_bytes(limit);
}

uint32_t* v8__ResourceConstraints__stack_limit(
    const v8::ResourceConstraints* constraints) {
  return constraints->stack_limit();
}

void v8__ResourceConstraints__set_stack_limit(
    v8::ResourceConstraints* constraints, uint32_t* value) {
  constraints->set_stack_limit(value);
}

size_t v8__ResourceConstraints__initial_old_generation_size_in_bytes(
    const v8::ResourceConstraints* constraints) {
  return constraints->initial_old_generation_size_in_bytes();
}

void v8__ResourceConstraints__set_initial_old_generation_size_in_bytes(
    v8::ResourceConstraints* constraints, size_t initial_size) {
  constraints->set_initial_old_generation_size_in_bytes(initial_size);
}

size_t v8__ResourceConstraints__initial_young_generation_size_in_bytes(
    const v8::ResourceConstraints* constraints) {
  return constraints->initial_young_generation_size_in_bytes();
}

void v8__ResourceConstraints__set_initial_young_generation_size_in_bytes(
    v8::ResourceConstraints* constraints, size_t initial_size) {
  constraints->set_initial_young_generation_size_in_bytes(initial_size);
}

void v8__HandleScope__CONSTRUCT(uninit_t<v8::HandleScope>* buf,
                                v8::Isolate* isolate) {
  construct_in_place<v8::HandleScope>(buf, isolate);
}

void v8__HandleScope__DESTRUCT(v8::HandleScope* self) { self->~HandleScope(); }

const v8::Data* v8__Local__New(v8::Isolate* isolate, const v8::Data& other) {
  return local_to_ptr(v8::Local<v8::Data>::New(isolate, ptr_to_local(&other)));
}

const v8::Data* v8__Global__New(v8::Isolate* isolate, const v8::Data& other) {
  // We have to use `std::move()` here because v8 disables the copy constructor
  // for class `v8::Global`.
  auto global = v8::Global<v8::Data>(isolate, ptr_to_local(&other));
  return make_pod<v8::Data*>(std::move(global));
}

const v8::Data* v8__Global__NewWeak(
    v8::Isolate* isolate, const v8::Data& other, void* parameter,
    v8::WeakCallbackInfo<void>::Callback callback) {
  auto global = v8::Global<v8::Data>(isolate, ptr_to_local(&other));
  global.SetWeak(parameter, callback, v8::WeakCallbackType::kParameter);
  return make_pod<v8::Data*>(std::move(global));
}

void v8__Global__Reset(const v8::Data* data) {
  auto global = ptr_to_global(data);
  global.Reset();
}

void v8__TracedReference__CONSTRUCT(
    uninit_t<v8::TracedReference<v8::Data>>* buf) {
  construct_in_place<v8::TracedReference<v8::Data>>(buf);
}

void v8__TracedReference__DESTRUCT(v8::TracedReference<v8::Data>* self) {
  self->~TracedReference();
}

void v8__TracedReference__Reset(v8::TracedReference<v8::Data>* self,
                                v8::Isolate* isolate, const v8::Data* other) {
  self->Reset(isolate, ptr_to_local(other));
}

const v8::Data* v8__TracedReference__Get(v8::TracedReference<v8::Data>* self,
                                         v8::Isolate* isolate) {
  return local_to_ptr(self->Get(isolate));
}

void v8__Eternal__CONSTRUCT(uninit_t<v8::Eternal<v8::Data>>* buf) {
  construct_in_place<v8::Eternal<v8::Data>>(buf);
}

void v8__Eternal__DESTRUCT(v8::Eternal<v8::Data>* self) { self->~Eternal(); }

void v8__Eternal__Clear(v8::Eternal<v8::Data>* self) { self->Clear(); }

bool v8__Eternal__IsEmpty(v8::Eternal<v8::Data>* self) {
  return self->IsEmpty();
}

void v8__Eternal__Set(v8::Eternal<v8::Data>* self, v8::Isolate* isolate,
                      const v8::Data* value) {
  self->Set(isolate, ptr_to_local(value));
}

const v8::Data* v8__Eternal__Get(v8::Eternal<v8::Data>* self,
                                 v8::Isolate* isolate) {
  return local_to_ptr(self->Get(isolate));
}

v8::Isolate* v8__WeakCallbackInfo__GetIsolate(
    const v8::WeakCallbackInfo<void>* self) {
  return self->GetIsolate();
}

void* v8__WeakCallbackInfo__GetParameter(
    const v8::WeakCallbackInfo<void>* self) {
  return self->GetParameter();
}

void v8__WeakCallbackInfo__SetSecondPassCallback(
    const v8::WeakCallbackInfo<void>* self,
    v8::WeakCallbackInfo<void>::Callback callback) {
  self->SetSecondPassCallback(callback);
}

void v8__ScriptCompiler__Source__CONSTRUCT(
    uninit_t<v8::ScriptCompiler::Source>* buf, const v8::String& source_string,
    const v8::ScriptOrigin* origin,
    v8::ScriptCompiler::CachedData* cached_data) {
  if (origin) {
    construct_in_place<v8::ScriptCompiler::Source>(
        buf, ptr_to_local(&source_string), *origin, cached_data);
  } else {
    construct_in_place<v8::ScriptCompiler::Source>(
        buf, ptr_to_local(&source_string), cached_data);
  }
}

void v8__ScriptCompiler__Source__DESTRUCT(v8::ScriptCompiler::Source* self) {
  self->~Source();
}

v8::ScriptCompiler::CachedData* v8__ScriptCompiler__CachedData__NEW(
    const uint8_t* data, int length) {
  return new v8::ScriptCompiler::CachedData(
      data, length, v8::ScriptCompiler::CachedData::BufferNotOwned);
}

void v8__ScriptCompiler__CachedData__DELETE(
    v8::ScriptCompiler::CachedData* self) {
  delete self;
}

int v8__ScriptCompiler__CachedData__CompatibilityCheck(
    v8::ScriptCompiler::CachedData* self, v8::Isolate* isolate) {
  return static_cast<int>(self->CompatibilityCheck(isolate));
}

bool v8__ScriptCompiler__CompileOptionsIsValid(int compile_options) {
  return v8::ScriptCompiler::CompileOptionsIsValid(
      static_cast<v8::ScriptCompiler::CompileOptions>(compile_options));
}

const v8::ScriptCompiler::CachedData* v8__ScriptCompiler__Source__GetCachedData(
    const v8::ScriptCompiler::Source* source) {
  return source->GetCachedData();
}

const v8::Module* v8__ScriptCompiler__CompileModule(
    v8::Isolate* isolate, v8::ScriptCompiler::Source* source,
    v8::ScriptCompiler::CompileOptions options,
    v8::ScriptCompiler::NoCacheReason no_cache_reason) {
  v8::MaybeLocal<v8::Module> maybe_local = v8::ScriptCompiler::CompileModule(
      isolate, source, options, no_cache_reason);
  return maybe_local_to_ptr(maybe_local);
}

const v8::Script* v8__ScriptCompiler__Compile(
    const v8::Context* context, v8::ScriptCompiler::Source* source,
    v8::ScriptCompiler::CompileOptions options,
    v8::ScriptCompiler::NoCacheReason no_cache_reason) {
  v8::MaybeLocal<v8::Script> maybe_local = v8::ScriptCompiler::Compile(
      ptr_to_local(context), source, options, no_cache_reason);
  return maybe_local_to_ptr(maybe_local);
}

const v8::Function* v8__ScriptCompiler__CompileFunction(
    const v8::Context* context, v8::ScriptCompiler::Source* source,
    size_t arguments_count, const v8::String** arguments,
    size_t context_extensions_count, const v8::Object** context_extensions,
    v8::ScriptCompiler::CompileOptions options,
    v8::ScriptCompiler::NoCacheReason no_cache_reason) {
  return maybe_local_to_ptr(v8::ScriptCompiler::CompileFunction(
      ptr_to_local(context), source, arguments_count,
      reinterpret_cast<v8::Local<v8::String>*>(arguments),
      context_extensions_count,
      reinterpret_cast<v8::Local<v8::Object>*>(context_extensions), options,
      no_cache_reason));
}

const v8::UnboundScript* v8__ScriptCompiler__CompileUnboundScript(
    v8::Isolate* isolate, v8::ScriptCompiler::Source* source,
    v8::ScriptCompiler::CompileOptions options,
    v8::ScriptCompiler::NoCacheReason no_cache_reason) {
  v8::MaybeLocal<v8::UnboundScript> maybe_local =
      v8::ScriptCompiler::CompileUnboundScript(isolate, source, options,
                                               no_cache_reason);
  return maybe_local_to_ptr(maybe_local);
}

uint32_t v8__ScriptCompiler__CachedDataVersionTag() {
  return v8::ScriptCompiler::CachedDataVersionTag();
}

size_t v8__TypedArray__Length(const v8::TypedArray* self) {
  return ptr_to_local(self)->Length();
}

bool v8__Data__EQ(const v8::Data& self, const v8::Data& other) {
  return ptr_to_local(&self) == ptr_to_local(&other);
}

bool v8__Data__IsBigInt(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsBigInt();
}

bool v8__Data__IsBoolean(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsBoolean();
}

bool v8__Data__IsContext(const v8::Data& self) { return self.IsContext(); }

bool v8__Data__IsFixedArray(const v8::Data& self) {
  return self.IsFixedArray();
}

bool v8__Data__IsFunctionTemplate(const v8::Data& self) {
  return self.IsFunctionTemplate();
}

bool v8__Data__IsModule(const v8::Data& self) { return self.IsModule(); }

bool v8__Data__IsModuleRequest(const v8::Data& self) {
  return self.IsModuleRequest();
}

bool v8__Data__IsName(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsName();
}

bool v8__Data__IsNumber(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsNumber();
}

bool v8__Data__IsObjectTemplate(const v8::Data& self) {
  return self.IsObjectTemplate();
}

bool v8__Data__IsDictionaryTemplate(const v8::Data& self) {
  return self.IsDictionaryTemplate();
}

bool v8__Data__IsPrimitive(const v8::Data& self) {
  return self.IsValue() &&
         reinterpret_cast<const v8::Value&>(self).IsPrimitive();
}

bool v8__Data__IsPrivate(const v8::Data& self) { return self.IsPrivate(); }

bool v8__Data__IsString(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsString();
}

bool v8__Data__IsSymbol(const v8::Data& self) {
  return self.IsValue() && reinterpret_cast<const v8::Value&>(self).IsSymbol();
}

bool v8__Data__IsValue(const v8::Data& self) { return self.IsValue(); }

bool v8__Value__IsUndefined(const v8::Value& self) {
  return self.IsUndefined();
}

bool v8__Value__IsNull(const v8::Value& self) { return self.IsNull(); }

bool v8__Value__IsNullOrUndefined(const v8::Value& self) {
  return self.IsNullOrUndefined();
}

bool v8__Value__IsTrue(const v8::Value& self) { return self.IsTrue(); }

bool v8__Value__IsFalse(const v8::Value& self) { return self.IsFalse(); }

bool v8__Value__IsName(const v8::Value& self) { return self.IsName(); }

bool v8__Value__IsString(const v8::Value& self) { return self.IsString(); }

bool v8__Value__IsSymbol(const v8::Value& self) { return self.IsSymbol(); }

bool v8__Value__IsFunction(const v8::Value& self) { return self.IsFunction(); }

bool v8__Value__IsArray(const v8::Value& self) { return self.IsArray(); }

bool v8__Value__IsObject(const v8::Value& self) { return self.IsObject(); }

bool v8__Value__IsBigInt(const v8::Value& self) { return self.IsBigInt(); }

bool v8__Value__IsBoolean(const v8::Value& self) { return self.IsBoolean(); }

bool v8__Value__IsNumber(const v8::Value& self) { return self.IsNumber(); }

bool v8__Value__IsExternal(const v8::Value& self) { return self.IsExternal(); }

bool v8__Value__IsInt32(const v8::Value& self) { return self.IsInt32(); }

bool v8__Value__IsUint32(const v8::Value& self) { return self.IsUint32(); }

bool v8__Value__IsDate(const v8::Value& self) { return self.IsDate(); }

bool v8__Value__IsArgumentsObject(const v8::Value& self) {
  return self.IsArgumentsObject();
}

bool v8__Value__IsBigIntObject(const v8::Value& self) {
  return self.IsBigIntObject();
}

bool v8__Value__IsBooleanObject(const v8::Value& self) {
  return self.IsBooleanObject();
}

bool v8__Value__IsNumberObject(const v8::Value& self) {
  return self.IsNumberObject();
}

bool v8__Value__IsStringObject(const v8::Value& self) {
  return self.IsStringObject();
}

bool v8__Value__IsSymbolObject(const v8::Value& self) {
  return self.IsSymbolObject();
}

bool v8__Value__IsNativeError(const v8::Value& self) {
  return self.IsNativeError();
}

bool v8__Value__IsRegExp(const v8::Value& self) { return self.IsRegExp(); }

bool v8__Value__IsAsyncFunction(const v8::Value& self) {
  return self.IsAsyncFunction();
}

bool v8__Value__IsGeneratorFunction(const v8::Value& self) {
  return self.IsGeneratorFunction();
}

bool v8__Value__IsGeneratorObject(const v8::Value& self) {
  return self.IsGeneratorObject();
}

bool v8__Value__IsPromise(const v8::Value& self) { return self.IsPromise(); }

bool v8__Value__IsMap(const v8::Value& self) { return self.IsMap(); }

bool v8__Value__IsSet(const v8::Value& self) { return self.IsSet(); }

bool v8__Value__IsMapIterator(const v8::Value& self) {
  return self.IsMapIterator();
}

bool v8__Value__IsSetIterator(const v8::Value& self) {
  return self.IsSetIterator();
}

bool v8__Value__IsSetGeneratorObject(const v8::Value& self) {
  return self.IsGeneratorObject();
}

bool v8__Value__IsWeakMap(const v8::Value& self) { return self.IsWeakMap(); }

bool v8__Value__IsWeakSet(const v8::Value& self) { return self.IsWeakSet(); }

bool v8__Value__IsWeakRef(const v8::Value& self) { return self.IsWeakRef(); }

bool v8__Value__IsWasmNull(const v8::Value& self) { return self.IsWasmNull(); }

bool v8__Value__IsArrayBuffer(const v8::Value& self) {
  return self.IsArrayBuffer();
}

bool v8__Value__IsArrayBufferView(const v8::Value& self) {
  return self.IsArrayBufferView();
}

bool v8__Value__IsTypedArray(const v8::Value& self) {
  return self.IsTypedArray();
}

bool v8__Value__IsUint8Array(const v8::Value& self) {
  return self.IsUint8Array();
}

bool v8__Value__IsUint8ClampedArray(const v8::Value& self) {
  return self.IsUint8ClampedArray();
}

bool v8__Value__IsInt8Array(const v8::Value& self) {
  return self.IsInt8Array();
}

bool v8__Value__IsUint16Array(const v8::Value& self) {
  return self.IsUint16Array();
}

bool v8__Value__IsInt16Array(const v8::Value& self) {
  return self.IsInt16Array();
}

bool v8__Value__IsUint32Array(const v8::Value& self) {
  return self.IsUint32Array();
}

bool v8__Value__IsInt32Array(const v8::Value& self) {
  return self.IsInt32Array();
}

bool v8__Value__IsFloat16Array(const v8::Value& self) {
  return self.IsFloat16Array();
}

bool v8__Value__IsFloat32Array(const v8::Value& self) {
  return self.IsFloat32Array();
}

bool v8__Value__IsFloat64Array(const v8::Value& self) {
  return self.IsFloat64Array();
}

bool v8__Value__IsBigInt64Array(const v8::Value& self) {
  return self.IsBigInt64Array();
}

bool v8__Value__IsBigUint64Array(const v8::Value& self) {
  return self.IsBigUint64Array();
}

bool v8__Value__IsDataView(const v8::Value& self) { return self.IsDataView(); }

bool v8__Value__IsSharedArrayBuffer(const v8::Value& self) {
  return self.IsSharedArrayBuffer();
}

bool v8__Value__IsProxy(const v8::Value& self) { return self.IsProxy(); }

bool v8__Value__IsWasmModuleObject(const v8::Value& self) {
  return self.IsWasmModuleObject();
}

bool v8__Value__IsWasmMemoryObject(const v8::Value& self) {
  return self.IsWasmMemoryObject();
}

bool v8__Value__IsModuleNamespaceObject(const v8::Value& self) {
  return self.IsModuleNamespaceObject();
}

bool v8__Value__StrictEquals(const v8::Value& self, const v8::Value& that) {
  return self.StrictEquals(ptr_to_local(&that));
}

bool v8__Value__SameValue(const v8::Value& self, const v8::Value& that) {
  return self.SameValue(ptr_to_local(&that));
}

const v8::Uint32* v8__Value__ToUint32(const v8::Value& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(self.ToUint32(ptr_to_local(&context)));
}

const v8::Int32* v8__Value__ToInt32(const v8::Value& self,
                                    const v8::Context& context) {
  return maybe_local_to_ptr(self.ToInt32(ptr_to_local(&context)));
}

const v8::Integer* v8__Value__ToInteger(const v8::Value& self,
                                        const v8::Context& context) {
  return maybe_local_to_ptr(self.ToInteger(ptr_to_local(&context)));
}

const v8::BigInt* v8__Value__ToBigInt(const v8::Value& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(self.ToBigInt(ptr_to_local(&context)));
}

const v8::String* v8__Value__ToString(const v8::Value& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(self.ToString(ptr_to_local(&context)));
}

const v8::String* v8__Value__ToDetailString(const v8::Value& self,
                                            const v8::Context& context) {
  return maybe_local_to_ptr(self.ToDetailString(ptr_to_local(&context)));
}

const v8::Number* v8__Value__ToNumber(const v8::Value& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(self.ToNumber(ptr_to_local(&context)));
}

const v8::Object* v8__Value__ToObject(const v8::Value& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(self.ToObject(ptr_to_local(&context)));
}

const v8::Boolean* v8__Value__ToBoolean(const v8::Value& self,
                                        v8::Isolate* isolate) {
  return local_to_ptr(self.ToBoolean(isolate));
}

void v8__Value__InstanceOf(const v8::Value& self, const v8::Context& context,
                           const v8::Object& object, v8::Maybe<bool>* out) {
  v8::Value* self_non_const = const_cast<v8::Value*>(&self);
  *out =
      self_non_const->InstanceOf(ptr_to_local(&context), ptr_to_local(&object));
}

void v8__Value__NumberValue(const v8::Value& self, const v8::Context& context,
                            v8::Maybe<double>* out) {
  *out = self.NumberValue(ptr_to_local(&context));
}

void v8__Value__IntegerValue(const v8::Value& self, const v8::Context& context,
                             v8::Maybe<int64_t>* out) {
  *out = self.IntegerValue(ptr_to_local(&context));
}

void v8__Value__Uint32Value(const v8::Value& self, const v8::Context& context,
                            v8::Maybe<uint32_t>* out) {
  *out = self.Uint32Value(ptr_to_local(&context));
}

void v8__Value__Int32Value(const v8::Value& self, const v8::Context& context,
                           v8::Maybe<int32_t>* out) {
  *out = self.Int32Value(ptr_to_local(&context));
}

bool v8__Value__BooleanValue(const v8::Value& self, v8::Isolate* isolate) {
  return self.BooleanValue(isolate);
}

const v8::String* v8__Value__TypeOf(v8::Value& self, v8::Isolate* isolate) {
  return local_to_ptr(self.TypeOf(isolate));
}

uint32_t v8__Value__GetHash(v8::Value& self) { return self.GetHash(); }

const v8::Primitive* v8__Null(v8::Isolate* isolate) {
  return local_to_ptr(v8::Null(isolate));
}

const v8::Primitive* v8__Undefined(v8::Isolate* isolate) {
  return local_to_ptr(v8::Undefined(isolate));
}

const v8::Boolean* v8__Boolean__New(v8::Isolate* isolate, bool value) {
  return local_to_ptr(v8::Boolean::New(isolate, value));
}

bool v8__Boolean__Value(const v8::Boolean& self) { return self.Value(); }

int v8__FixedArray__Length(const v8::FixedArray& self) { return self.Length(); }

const v8::Data* v8__FixedArray__Get(const v8::FixedArray& self, int index) {
  return local_to_ptr(ptr_to_local(&self)->Get(index));
}

const v8::PrimitiveArray* v8__PrimitiveArray__New(v8::Isolate* isolate,
                                                  int length) {
  return local_to_ptr(v8::PrimitiveArray::New(isolate, length));
}

int v8__PrimitiveArray__Length(const v8::PrimitiveArray& self) {
  return self.Length();
}

void v8__PrimitiveArray__Set(const v8::PrimitiveArray& self,
                             v8::Isolate* isolate, int index,
                             const v8::Primitive& item) {
  ptr_to_local(&self)->Set(isolate, index, ptr_to_local(&item));
}

const v8::Primitive* v8__PrimitiveArray__Get(const v8::PrimitiveArray& self,
                                             v8::Isolate* isolate, int index) {
  return local_to_ptr(ptr_to_local(&self)->Get(isolate, index));
}

v8::BackingStore* v8__ArrayBuffer__NewBackingStore__with_byte_length(
    v8::Isolate* isolate, size_t byte_length) {
  std::unique_ptr<v8::BackingStore> u =
      v8::ArrayBuffer::NewBackingStore(isolate, byte_length);
  return u.release();
}

v8::BackingStore* v8__ArrayBuffer__NewBackingStore__with_mode(
    v8::Isolate* isolate, size_t byte_length,
    v8::BackingStoreInitializationMode initialization_mode,
    v8::BackingStoreOnFailureMode on_failure) {
  std::unique_ptr<v8::BackingStore> u = v8::ArrayBuffer::NewBackingStore(
      isolate, byte_length, initialization_mode, on_failure);
  return u.release();
}

v8::BackingStore* v8__ArrayBuffer__NewResizableBackingStore(
    size_t byte_length, size_t max_byte_length) {
  std::unique_ptr<v8::BackingStore> u =
      v8::ArrayBuffer::NewResizableBackingStore(byte_length, max_byte_length);
  return u.release();
}

v8::BackingStore* v8__ArrayBuffer__NewBackingStore__with_data(
    void* data, size_t byte_length, v8::BackingStore::DeleterCallback deleter,
    void* deleter_data) {
  std::unique_ptr<v8::BackingStore> u = v8::ArrayBuffer::NewBackingStore(
      data, byte_length, deleter, deleter_data);
  return u.release();
}

two_pointers_t v8__ArrayBuffer__GetBackingStore(const v8::ArrayBuffer& self) {
  return make_pod<two_pointers_t>(ptr_to_local(&self)->GetBackingStore());
}

bool v8__BackingStore__IsResizableByUserJavaScript(
    const v8::BackingStore& self) {
  return ptr_to_local(&self)->IsResizableByUserJavaScript();
}

void* v8__ArrayBuffer__Data(const v8::ArrayBuffer& self) {
  return ptr_to_local(&self)->Data();
}

MaybeBool v8__ArrayBuffer__Detach(const v8::ArrayBuffer& self,
                                  const v8::Value* key) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->Detach(ptr_to_local(key)));
}

bool v8__ArrayBuffer__IsDetachable(const v8::ArrayBuffer& self) {
  return ptr_to_local(&self)->IsDetachable();
}

bool v8__ArrayBuffer__WasDetached(const v8::ArrayBuffer& self) {
  return ptr_to_local(&self)->WasDetached();
}

bool v8__ArrayBuffer__IsResizableByUserJavaScript(const v8::ArrayBuffer& self) {
  return ptr_to_local(&self)->IsResizableByUserJavaScript();
}

bool v8__ArrayBuffer__IsImmutable(const v8::ArrayBuffer& self) {
  return ptr_to_local(&self)->IsImmutable();
}

void v8__ArrayBuffer__SetDetachKey(const v8::ArrayBuffer& self,
                                   const v8::Value* key) {
  return ptr_to_local(&self)->SetDetachKey(ptr_to_local(key));
}

void* v8__BackingStore__Data(const v8::BackingStore& self) {
  return self.Data();
}

size_t v8__BackingStore__ByteLength(const v8::BackingStore& self) {
  return self.ByteLength();
}

size_t v8__BackingStore__MaxByteLength(const v8::BackingStore& self) {
  return self.MaxByteLength();
}

bool v8__BackingStore__IsShared(const v8::BackingStore& self) {
  return self.IsShared();
}

void v8__BackingStore__DELETE(v8::BackingStore* self) { delete self; }

two_pointers_t std__shared_ptr__v8__BackingStore__COPY(
    const std::shared_ptr<v8::BackingStore>& ptr) {
  return make_pod<two_pointers_t>(ptr);
}

two_pointers_t std__shared_ptr__v8__BackingStore__CONVERT__std__unique_ptr(
    v8::BackingStore* unique_ptr) {
  return make_pod<two_pointers_t>(
      std::shared_ptr<v8::BackingStore>(unique_ptr));
}

v8::BackingStore* std__shared_ptr__v8__BackingStore__get(
    const std::shared_ptr<v8::BackingStore>& ptr) {
  return ptr.get();
}

void std__shared_ptr__v8__BackingStore__reset(
    std::shared_ptr<v8::BackingStore>* ptr) {
  ptr->reset();
}

long std__shared_ptr__v8__BackingStore__use_count(
    const std::shared_ptr<v8::BackingStore>& ptr) {
  return ptr.use_count();
}

two_pointers_t std__shared_ptr__v8__ArrayBuffer__Allocator__COPY(
    const std::shared_ptr<v8::ArrayBuffer::Allocator>& ptr) {
  return make_pod<two_pointers_t>(ptr);
}

two_pointers_t
std__shared_ptr__v8__ArrayBuffer__Allocator__CONVERT__std__unique_ptr(
    v8::ArrayBuffer::Allocator* unique_ptr) {
  return make_pod<two_pointers_t>(
      std::shared_ptr<v8::ArrayBuffer::Allocator>(unique_ptr));
}

v8::ArrayBuffer::Allocator* std__shared_ptr__v8__ArrayBuffer__Allocator__get(
    const std::shared_ptr<v8::ArrayBuffer::Allocator>& ptr) {
  return ptr.get();
}

void std__shared_ptr__v8__ArrayBuffer__Allocator__reset(
    std::shared_ptr<v8::ArrayBuffer::Allocator>* ptr) {
  ptr->reset();
}

long std__shared_ptr__v8__ArrayBuffer__Allocator__use_count(
    const std::shared_ptr<v8::ArrayBuffer::Allocator>& ptr) {
  return ptr.use_count();
}

int v8__Name__GetIdentityHash(const v8::Name& self) {
  return ptr_to_local(&self)->GetIdentityHash();
}

const v8::String* v8__String__Empty(v8::Isolate* isolate) {
  return local_to_ptr(v8::String::Empty(isolate));
}

const v8::String* v8__String__NewFromUtf8(v8::Isolate* isolate,
                                          const char* data,
                                          v8::NewStringType new_type,
                                          int length) {
  return maybe_local_to_ptr(
      v8::String::NewFromUtf8(isolate, data, new_type, length));
}

const v8::String* v8__String__NewFromOneByte(v8::Isolate* isolate,
                                             const uint8_t* data,
                                             v8::NewStringType new_type,
                                             int length) {
  return maybe_local_to_ptr(
      v8::String::NewFromOneByte(isolate, data, new_type, length));
}

const v8::String* v8__String__NewFromTwoByte(v8::Isolate* isolate,
                                             const uint16_t* data,
                                             v8::NewStringType new_type,
                                             int length) {
  return maybe_local_to_ptr(
      v8::String::NewFromTwoByte(isolate, data, new_type, length));
}

const v8::String* v8__String__Concat(v8::Isolate* isolate,
                                     const v8::String& left,
                                     const v8::String& right) {
  return local_to_ptr(
      v8::String::Concat(isolate, ptr_to_local(&left), ptr_to_local(&right)));
}

int v8__String__Length(const v8::String& self) { return self.Length(); }

int v8__String__Utf8Length(const v8::String& self, v8::Isolate* isolate) {
  return self.Utf8LengthV2(isolate);
}

void v8__String__Write_v2(const v8::String& self, v8::Isolate* isolate,
                          uint32_t offset, uint32_t length, uint16_t* buffer,
                          int flags) {
  return self.WriteV2(isolate, offset, length, buffer, flags);
}

void v8__String__WriteOneByte_v2(const v8::String& self, v8::Isolate* isolate,
                                 uint32_t offset, uint32_t length,
                                 uint8_t* buffer, int flags) {
  return self.WriteOneByteV2(isolate, offset, length, buffer, flags);
}

size_t v8__String__WriteUtf8_v2(const v8::String& self, v8::Isolate* isolate,
                                char* buffer, size_t capacity, int flags,
                                size_t* processed_characters_return) {
  return self.WriteUtf8V2(isolate, buffer, capacity, flags,
                          processed_characters_return);
}

const v8::String::ExternalStringResource* v8__String__GetExternalStringResource(
    const v8::String& self) {
  return self.GetExternalStringResource();
}

const v8::String::ExternalStringResourceBase*
v8__String__GetExternalStringResourceBase(const v8::String& self,
                                          v8::String::Encoding* encoding_out) {
  return self.GetExternalStringResourceBase(encoding_out);
}

class ExternalOneByteString : public v8::String::ExternalOneByteStringResource {
 public:
  using RustDestroy = void (*)(char*, size_t);
  ExternalOneByteString(char* data, size_t length, RustDestroy rustDestroy,
                        v8::Isolate* isolate)
      : data_(data),
        length_(length),
        rustDestroy_(rustDestroy),
        isolate_(isolate) {
    isolate_->AdjustAmountOfExternalAllocatedMemory(
        static_cast<int64_t>(length_));
  }
  ~ExternalOneByteString() override {
    (*rustDestroy_)(data_, length_);
    isolate_->AdjustAmountOfExternalAllocatedMemory(
        -static_cast<int64_t>(length_));
  }

  const char* data() const override { return data_; }

  size_t length() const override { return length_; }

 private:
  char* data_;
  const size_t length_;
  RustDestroy rustDestroy_;
  v8::Isolate* isolate_;
};

class ExternalStaticOneByteStringResource
    : public v8::String::ExternalOneByteStringResource {
 public:
  ExternalStaticOneByteStringResource(const char* data, int length)
      : _data(data), _length(length) {}
  const char* data() const override { return _data; }
  size_t length() const override { return _length; }

 private:
  const char* _data;
  const int _length;
};

const v8::String* v8__String__NewExternalOneByteConst(
    v8::Isolate* isolate, v8::String::ExternalOneByteStringResource* resource) {
  return maybe_local_to_ptr(v8::String::NewExternalOneByte(isolate, resource));
}

const v8::String* v8__String__NewExternalOneByteStatic(v8::Isolate* isolate,
                                                       const char* data,
                                                       int length) {
  return maybe_local_to_ptr(v8::String::NewExternalOneByte(
      isolate, new ExternalStaticOneByteStringResource(data, length)));
}

const v8::String* v8__String__NewExternalOneByte(
    v8::Isolate* isolate, char* data, size_t length,
    ExternalOneByteString::RustDestroy rustDestroy) {
  return maybe_local_to_ptr(v8::String::NewExternalOneByte(
      isolate, new ExternalOneByteString(data, length, rustDestroy, isolate)));
}

const char* v8__ExternalOneByteStringResource__data(
    v8::String::ExternalOneByteStringResource* self) {
  return self->data();
}

size_t v8__ExternalOneByteStringResource__length(
    v8::String::ExternalOneByteStringResource* self) {
  return self->length();
}

class ExternalTwoByteString : public v8::String::ExternalStringResource {
 public:
  using RustDestroy = void (*)(uint16_t*, size_t);
  ExternalTwoByteString(uint16_t* data, size_t length, RustDestroy rustDestroy,
                        v8::Isolate* isolate)
      : data_(data),
        length_(length),
        rustDestroy_(rustDestroy),
        isolate_(isolate) {
    isolate_->AdjustAmountOfExternalAllocatedMemory(
        static_cast<int64_t>(length_ * 2));
  }
  ~ExternalTwoByteString() override {
    (*rustDestroy_)(data_, length_);
    isolate_->AdjustAmountOfExternalAllocatedMemory(
        -static_cast<int64_t>(length_ * 2));
  }

  const uint16_t* data() const override { return data_; }

  size_t length() const override { return length_; }

 private:
  uint16_t* data_;
  const size_t length_;
  RustDestroy rustDestroy_;
  v8::Isolate* isolate_;
};

class ExternalStaticStringResource : public v8::String::ExternalStringResource {
 public:
  ExternalStaticStringResource(const uint16_t* data, int length)
      : _data(data), _length(length) {}
  const uint16_t* data() const override { return _data; }
  size_t length() const override { return _length; }

 private:
  const uint16_t* _data;
  const int _length;
};

const v8::String* v8__String__NewExternalTwoByteStatic(v8::Isolate* isolate,
                                                       const uint16_t* data,
                                                       int length) {
  return maybe_local_to_ptr(v8::String::NewExternalTwoByte(
      isolate, new ExternalStaticStringResource(data, length)));
}

const v8::String* v8__String__NewExternalTwoByte(
    v8::Isolate* isolate, uint16_t* data, size_t length,
    ExternalTwoByteString::RustDestroy rustDestroy) {
  return maybe_local_to_ptr(v8::String::NewExternalTwoByte(
      isolate, new ExternalTwoByteString(data, length, rustDestroy, isolate)));
}

bool v8__String__IsExternal(const v8::String& self) {
  return self.IsExternal();
}
bool v8__String__IsExternalOneByte(const v8::String& self) {
  return self.IsExternalOneByte();
}
bool v8__String__IsExternalTwoByte(const v8::String& self) {
  return self.IsExternalTwoByte();
}
bool v8__String__IsOneByte(const v8::String& self) { return self.IsOneByte(); }
bool v8__String__ContainsOnlyOneByte(const v8::String& self) {
  return self.ContainsOnlyOneByte();
}

bool v8__String__StringEquals(const v8::String& self, const v8::String* that) {
  return self.StringEquals(ptr_to_local(that));
}

const v8::String* v8__String__InternalizeString(v8::String& self,
                                                v8::Isolate* isolate) {
  return local_to_ptr(ptr_to_local(&self)->InternalizeString(isolate));
}

void v8__String__ValueView__CONSTRUCT(uninit_t<v8::String::ValueView>* buf,
                                      v8::Isolate* isolate,
                                      const v8::String& string) {
  construct_in_place<v8::String::ValueView>(buf, isolate,
                                            ptr_to_local(&string));
}

void v8__String__ValueView__DESTRUCT(v8::String::ValueView* self) {
  self->~ValueView();
}

const v8::Symbol* v8__Symbol__New(v8::Isolate* isolate,
                                  const v8::String* description) {
  return local_to_ptr(v8::Symbol::New(isolate, ptr_to_local(description)));
}

const v8::Symbol* v8__Symbol__For(v8::Isolate* isolate,
                                  const v8::String& description) {
  return local_to_ptr(v8::Symbol::For(isolate, ptr_to_local(&description)));
}

const v8::Symbol* v8__Symbol__ForApi(v8::Isolate* isolate,
                                     const v8::String& description) {
  return local_to_ptr(v8::Symbol::ForApi(isolate, ptr_to_local(&description)));
}

#define V(NAME)                                                   \
  const v8::Symbol* v8__Symbol__Get##NAME(v8::Isolate* isolate) { \
    return local_to_ptr(v8::Symbol::Get##NAME(isolate));          \
  }

V(AsyncIterator)
V(HasInstance)
V(IsConcatSpreadable)
V(Iterator)
V(Match)
V(Replace)
V(Search)
V(Split)
V(ToPrimitive)
V(ToStringTag)
V(Unscopables)
V(Dispose)
V(AsyncDispose)
#undef V

const v8::Value* v8__Symbol__Description(const v8::Symbol& self,
                                         v8::Isolate* isolate) {
  return local_to_ptr(ptr_to_local(&self)->Description(isolate));
}

const v8::Private* v8__Private__New(v8::Isolate* isolate,
                                    const v8::String* name) {
  return local_to_ptr(v8::Private::New(isolate, ptr_to_local(name)));
}

const v8::Private* v8__Private__ForApi(v8::Isolate* isolate,
                                       const v8::String* name) {
  return local_to_ptr(v8::Private::ForApi(isolate, ptr_to_local(name)));
}

const v8::Value* v8__Private__Name(const v8::Private& self) {
  return local_to_ptr(ptr_to_local(&self)->Name());
}

void v8__Template__Set(const v8::Template& self, const v8::Name& key,
                       const v8::Data& value, v8::PropertyAttribute attr) {
  ptr_to_local(&self)->Set(ptr_to_local(&key), ptr_to_local(&value), attr);
}

void v8__Template__SetIntrinsicDataProperty(const v8::Template& self,
                                            const v8::Name& key,
                                            v8::Intrinsic intrinsic,
                                            v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetIntrinsicDataProperty(ptr_to_local(&key), intrinsic,
                                                attr);
}

// Rust passes accessor setters with the old PropertyCallbackInfo<void>
// signature. V8 calls every setter through AccessorNameSetterCallbackV2 and
// treats an unset return value as success, so this cast matches what V8
// does internally (see PropertyCallbackArguments::CallAccessorSetter).
using RustAccessorNameSetterCallback =
    void (*)(v8::Local<v8::Name> property, v8::Local<v8::Value> value,
             const v8::PropertyCallbackInfo<void>& info);

static v8::AccessorNameSetterCallbackV2 to_setter_v2(
    RustAccessorNameSetterCallback setter) {
  return reinterpret_cast<v8::AccessorNameSetterCallbackV2>(setter);
}

void v8__Template__SetNativeDataProperty(const v8::Template& self,
                                         const v8::Name& key,
                                         v8::AccessorNameGetterCallback getter,
                                         RustAccessorNameSetterCallback setter,
                                         const v8::Value* data_or_null,
                                         v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetNativeDataProperty(ptr_to_local(&key), getter,
                                             to_setter_v2(setter),
                                             ptr_to_local(data_or_null), attr);
}

void v8__Template__SetLazyDataProperty(const v8::Template& self,
                                       const v8::Name& key,
                                       v8::AccessorNameGetterCallback getter,
                                       const v8::Value* data_or_null,
                                       v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetLazyDataProperty(ptr_to_local(&key), getter,
                                           ptr_to_local(data_or_null), attr);
}

const v8::ObjectTemplate* v8__ObjectTemplate__New(
    v8::Isolate* isolate, const v8::FunctionTemplate& templ) {
  return local_to_ptr(v8::ObjectTemplate::New(isolate, ptr_to_local(&templ)));
}

const v8::Object* v8__ObjectTemplate__NewInstance(
    const v8::ObjectTemplate& self, const v8::Context& context) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->NewInstance(ptr_to_local(&context)));
}

int v8__ObjectTemplate__InternalFieldCount(const v8::ObjectTemplate& self) {
  return ptr_to_local(&self)->InternalFieldCount();
}

void v8__ObjectTemplate__SetInternalFieldCount(const v8::ObjectTemplate& self,
                                               int value) {
  ptr_to_local(&self)->SetInternalFieldCount(value);
}

void v8__ObjectTemplate__SetNativeDataProperty(
    const v8::ObjectTemplate& self, const v8::Name& key,
    v8::AccessorNameGetterCallback getter,
    RustAccessorNameSetterCallback setter, const v8::Value* data_or_null,
    v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetNativeDataProperty(ptr_to_local(&key), getter,
                                             to_setter_v2(setter),
                                             ptr_to_local(data_or_null), attr);
}

void v8__ObjectTemplate__SetNamedPropertyHandler(
    const v8::ObjectTemplate& self, v8::NamedPropertyGetterCallback getter,
    v8::NamedPropertySetterCallbackV2 setter,
    v8::NamedPropertyQueryCallback query,
    v8::NamedPropertyDeleterCallback deleter,
    v8::NamedPropertyEnumeratorCallback enumerator,
    v8::NamedPropertyDefinerCallbackV2 definer,
    v8::NamedPropertyDescriptorCallback descriptor,
    const v8::Value* data_or_null, v8::PropertyHandlerFlags flags) {
  ptr_to_local(&self)->SetHandler(v8::NamedPropertyHandlerConfiguration(
      getter, setter, query, deleter, enumerator, definer, descriptor,
      ptr_to_local(data_or_null), flags));
}

void v8__ObjectTemplate__SetIndexedPropertyHandler(
    const v8::ObjectTemplate& self, v8::IndexedPropertyGetterCallbackV2 getter,
    v8::IndexedPropertySetterCallback setter,
    v8::IndexedPropertyQueryCallbackV2 query,
    v8::IndexedPropertyDeleterCallbackV2 deleter,
    v8::IndexedPropertyEnumeratorCallback enumerator,
    v8::IndexedPropertyDefinerCallback definer,
    v8::IndexedPropertyDescriptorCallbackV2 descriptor,
    const v8::Value* data_or_null, v8::PropertyHandlerFlags flags) {
  ptr_to_local(&self)->SetHandler(v8::IndexedPropertyHandlerConfiguration(
      getter, setter, query, deleter, enumerator, definer, descriptor,
      ptr_to_local(data_or_null), flags));
}

void v8__ObjectTemplate__SetAccessorProperty(const v8::ObjectTemplate& self,
                                             const v8::Name& key,
                                             v8::FunctionTemplate& getter,
                                             v8::FunctionTemplate& setter,
                                             v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetAccessorProperty(
      ptr_to_local(&key), ptr_to_local(&getter), ptr_to_local(&setter), attr);
}

void v8__ObjectTemplate__SetImmutableProto(const v8::ObjectTemplate& self) {
  return ptr_to_local(&self)->SetImmutableProto();
}

void v8__ObjectTemplate__SetCallAsFunctionHandler(
    const v8::ObjectTemplate& self, v8::FunctionCallback callback,
    const v8::Value* data_or_null) {
  ptr_to_local(&self)->SetCallAsFunctionHandler(callback,
                                                ptr_to_local(data_or_null));
}

const v8::Object* v8__Object__New(v8::Isolate* isolate) {
  return local_to_ptr(v8::Object::New(isolate));
}

const v8::Object* v8__Object__New__with_prototype_and_properties(
    v8::Isolate* isolate, const v8::Value& prototype_or_null,
    const v8::Name* const names[], const v8::Value* const values[],
    size_t length) {
  return local_to_ptr(v8::Object::New(isolate, ptr_to_local(&prototype_or_null),
                                      const_ptr_array_to_local_array(names),
                                      const_ptr_array_to_local_array(values),
                                      length));
}

const v8::Value* v8__Object__Get(const v8::Object& self,
                                 const v8::Context& context,
                                 const v8::Value& key) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Get(ptr_to_local(&context), ptr_to_local(&key)));
}

const v8::Value* v8__Object__GetWithReceiver(const v8::Object& self,
                                             const v8::Context& context,
                                             const v8::Value& key,
                                             const v8::Object& receiver) {
  return maybe_local_to_ptr(ptr_to_local(&self)->Get(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&receiver)));
}

const v8::Value* v8__Object__GetIndex(const v8::Object& self,
                                      const v8::Context& context,
                                      uint32_t index) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Get(ptr_to_local(&context), index));
}

void* v8__Object__GetAlignedPointerFromInternalField(
    const v8::Object& self, int index, v8::EmbedderDataTypeTag tag) {
  return ptr_to_local(&self)->GetAlignedPointerFromInternalField(index, tag);
}

void v8__Object__SetAlignedPointerInInternalField(const v8::Object& self,
                                                  int index, void* value,
                                                  v8::EmbedderDataTypeTag tag) {
  ptr_to_local(&self)->SetAlignedPointerInInternalField(index, value, tag);
}

bool v8__Object__IsApiWrapper(const v8::Object& self) {
  return ptr_to_local(&self)->IsApiWrapper();
}

const v8::Value* v8__Object__GetPrototype(const v8::Object& self) {
  return local_to_ptr(ptr_to_local(&self)->GetPrototype());
}

const v8::Object* v8__Object__FindInstanceInPrototypeChain(
    const v8::Object& self, const v8::FunctionTemplate& tmpl) {
  return local_to_ptr(
      ptr_to_local(&self)->FindInstanceInPrototypeChain(ptr_to_local(&tmpl)));
}

MaybeBool v8__Object__Set(const v8::Object& self, const v8::Context& context,
                          const v8::Value& key, const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->Set(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&value)));
}

MaybeBool v8__Object__SetWithReceiver(const v8::Object& self,
                                      const v8::Context& context,
                                      const v8::Value& key,
                                      const v8::Value& value,
                                      const v8::Object& receiver) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Set(ptr_to_local(&context), ptr_to_local(&key),
                               ptr_to_local(&value), ptr_to_local(&receiver)));
}

MaybeBool v8__Object__SetIndex(const v8::Object& self,
                               const v8::Context& context, uint32_t index,
                               const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->Set(
      ptr_to_local(&context), index, ptr_to_local(&value)));
}

MaybeBool v8__Object__SetPrototype(const v8::Object& self,
                                   const v8::Context& context,
                                   const v8::Value& prototype) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->SetPrototype(
      ptr_to_local(&context), ptr_to_local(&prototype)));
}

const v8::String* v8__Object__GetConstructorName(v8::Object& self) {
  return local_to_ptr(self.GetConstructorName());
}

MaybeBool v8__Object__CreateDataProperty(const v8::Object& self,
                                         const v8::Context& context,
                                         const v8::Name& key,
                                         const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->CreateDataProperty(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&value)));
}

MaybeBool v8__Object__DefineOwnProperty(const v8::Object& self,
                                        const v8::Context& context,
                                        const v8::Name& key,
                                        const v8::Value& value,
                                        v8::PropertyAttribute attr) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->DefineOwnProperty(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&value), attr));
}

MaybeBool v8__Object__DefineProperty(const v8::Object& self,
                                     const v8::Context& context,
                                     const v8::Name& key,
                                     v8::PropertyDescriptor& desc) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->DefineProperty(
      ptr_to_local(&context), ptr_to_local(&key), desc));
}

MaybeBool v8__Object__SetAccessor(const v8::Object& self,
                                  const v8::Context& context,
                                  const v8::Name& key,
                                  v8::AccessorNameGetterCallback getter,
                                  RustAccessorNameSetterCallback setter,
                                  const v8::Value* data_or_null,
                                  v8::PropertyAttribute attr) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->SetNativeDataProperty(
      ptr_to_local(&context), ptr_to_local(&key), getter, to_setter_v2(setter),
      ptr_to_local(data_or_null), attr));
}

MaybeBool v8__Object__SetLazyDataProperty(
    const v8::Object& self, const v8::Context& context, const v8::Name& key,
    v8::AccessorNameGetterCallback getter, const v8::Value* data_or_null,
    v8::PropertyAttribute attr, v8::SideEffectType getter_side_effect_type,
    v8::SideEffectType setter_side_effect_type) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->SetLazyDataProperty(
      ptr_to_local(&context), ptr_to_local(&key), getter,
      ptr_to_local(data_or_null), attr, getter_side_effect_type,
      setter_side_effect_type));
}

int v8__Object__GetIdentityHash(const v8::Object& self) {
  return ptr_to_local(&self)->GetIdentityHash();
}

const v8::Context* v8__Object__GetCreationContext(const v8::Object& self) {
  return maybe_local_to_ptr(ptr_to_local(&self)->GetCreationContext());
}

// v8::PropertyFilter
static_assert(v8::ALL_PROPERTIES == 0, "v8::ALL_PROPERTIES is not 0");
static_assert(v8::ONLY_WRITABLE == 1, "v8::ONLY_WRITABLE is not 1");
static_assert(v8::ONLY_ENUMERABLE == 2, "v8::ONLY_ENUMERABLE is not 2");
static_assert(v8::ONLY_CONFIGURABLE == 4, "v8::ONLY_CONFIGURABLE is not 4");
static_assert(v8::SKIP_STRINGS == 8, "v8::SKIP_STRINGS is not 8");
static_assert(v8::SKIP_SYMBOLS == 16, "v8::SKIP_SYMBOLS is not 16");

// v8::KeyConversionMode
static_assert(static_cast<int>(v8::KeyConversionMode::kConvertToString) == 0,
              "v8::KeyConversionMode::kConvertToString is not 0");
static_assert(static_cast<int>(v8::KeyConversionMode::kKeepNumbers) == 1,
              "v8::KeyConversionMode::kKeepNumbers is not 1");
static_assert(static_cast<int>(v8::KeyConversionMode::kNoNumbers) == 2,
              "v8::KeyConversionMode::kNoNumbers is not 2");

// v8::KeyCollectionMode
static_assert(static_cast<int>(v8::KeyCollectionMode::kOwnOnly) == 0,
              "v8::KeyCollectionMode::kOwnOnly is not 0");
static_assert(static_cast<int>(v8::KeyCollectionMode::kIncludePrototypes) == 1,
              "v8::KeyCollectionMode::kIncludePrototypes is not 1");

// v8::IndexFilter
static_assert(static_cast<int>(v8::IndexFilter::kIncludeIndices) == 0,
              "v8::IndexFilter::kIncludeIndices is not 0");
static_assert(static_cast<int>(v8::IndexFilter::kSkipIndices) == 1,
              "v8::IndexFilter::kSkipIndices is not 1");

const v8::Array* v8__Object__GetOwnPropertyNames(
    const v8::Object* self, const v8::Context* context,
    v8::PropertyFilter filter, v8::KeyConversionMode key_conversion) {
  return maybe_local_to_ptr(ptr_to_local(self)->GetOwnPropertyNames(
      ptr_to_local(context), filter, key_conversion));
}

const v8::Array* v8__Object__GetPropertyNames(
    const v8::Object* self, const v8::Context* context,
    v8::KeyCollectionMode mode, v8::PropertyFilter property_filter,
    v8::IndexFilter index_filter, v8::KeyConversionMode key_conversion) {
  return maybe_local_to_ptr(ptr_to_local(self)->GetPropertyNames(
      ptr_to_local(context), mode, property_filter, index_filter,
      key_conversion));
}

MaybeBool v8__Object__Has(const v8::Object& self, const v8::Context& context,
                          const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Has(ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__HasIndex(const v8::Object& self,
                               const v8::Context& context, uint32_t index) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Has(ptr_to_local(&context), index));
}

MaybeBool v8__Object__HasOwnProperty(const v8::Object& self,
                                     const v8::Context& context,
                                     const v8::Name& key) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->HasOwnProperty(
      ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__Delete(const v8::Object& self, const v8::Context& context,
                             const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Delete(ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__DeleteIndex(const v8::Object& self,
                                  const v8::Context& context, uint32_t index) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Delete(ptr_to_local(&context), index));
}

int v8__Object__InternalFieldCount(const v8::Object& self) {
  return ptr_to_local(&self)->InternalFieldCount();
}

const v8::Data* v8__Object__GetInternalField(const v8::Object& self,
                                             int index) {
  return local_to_ptr(ptr_to_local(&self)->GetInternalField(index));
}

static_assert(static_cast<int>(v8::IntegrityLevel::kFrozen) == 0,
              "v8::IntegrityLevel::kFrozen is not 0");
static_assert(static_cast<int>(v8::IntegrityLevel::kSealed) == 1,
              "v8::IntegrityLevel::kSealed is not 1");

MaybeBool v8__Object__SetIntegrityLevel(const v8::Object& self,
                                        const v8::Context& context,
                                        v8::IntegrityLevel level) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->SetIntegrityLevel(ptr_to_local(&context), level));
}

void v8__Object__SetInternalField(const v8::Object& self, int index,
                                  const v8::Data& data) {
  ptr_to_local(&self)->SetInternalField(index, ptr_to_local(&data));
}

const v8::Value* v8__Object__GetPrivate(const v8::Object& self,
                                        const v8::Context& context,
                                        const v8::Private& key) {
  return maybe_local_to_ptr(ptr_to_local(&self)->GetPrivate(
      ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__SetPrivate(const v8::Object& self,
                                 const v8::Context& context,
                                 const v8::Private& key,
                                 const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->SetPrivate(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&value)));
}

MaybeBool v8__Object__DeletePrivate(const v8::Object& self,
                                    const v8::Context& context,
                                    const v8::Private& key) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->DeletePrivate(
      ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__HasPrivate(const v8::Object& self,
                                 const v8::Context& context,
                                 const v8::Private& key) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->HasPrivate(
      ptr_to_local(&context), ptr_to_local(&key)));
}

void v8__Object__GetPropertyAttributes(const v8::Object& self,
                                       const v8::Context& context,
                                       const v8::Value& key,
                                       v8::Maybe<v8::PropertyAttribute>* out) {
  *out = ptr_to_local(&self)->GetPropertyAttributes(ptr_to_local(&context),
                                                    ptr_to_local(&key));
}

const v8::Value* v8__Object__GetOwnPropertyDescriptor(
    const v8::Object& self, const v8::Context& context, const v8::Name& key) {
  return maybe_local_to_ptr(ptr_to_local(&self)->GetOwnPropertyDescriptor(
      ptr_to_local(&context), ptr_to_local(&key)));
}

const v8::Value* v8__Object__GetRealNamedProperty(const v8::Object& self,
                                                  const v8::Context& context,
                                                  const v8::Name& key) {
  return maybe_local_to_ptr(ptr_to_local(&self)->GetRealNamedProperty(
      ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Object__HasRealNamedProperty(const v8::Object& self,
                                           const v8::Context& context,
                                           const v8::Name& key) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->HasRealNamedProperty(
      ptr_to_local(&context), ptr_to_local(&key)));
}

void v8__Object__GetRealNamedPropertyAttributes(
    const v8::Object& self, const v8::Context& context, const v8::Name& key,
    v8::Maybe<v8::PropertyAttribute>* out) {
  *out = ptr_to_local(&self)->GetRealNamedPropertyAttributes(
      ptr_to_local(&context), ptr_to_local(&key));
}

const v8::Array* v8__Object__PreviewEntries(const v8::Object& self,
                                            bool* is_key_value) {
  return maybe_local_to_ptr(ptr_to_local(&self)->PreviewEntries(is_key_value));
}

bool v8__Object__IsCallable(const v8::Object& self) {
  return ptr_to_local(&self)->IsCallable();
}

bool v8__Object__IsConstructor(const v8::Object& self) {
  return ptr_to_local(&self)->IsConstructor();
}

const v8::Value* v8__Object__CallAsFunction(const v8::Object& self,
                                            const v8::Context& context,
                                            const v8::Value& recv, int argc,
                                            const v8::Value* const argv[]) {
  return maybe_local_to_ptr(ptr_to_local(&self)->CallAsFunction(
      ptr_to_local(&context), ptr_to_local(&recv), argc,
      const_ptr_array_to_local_array(argv)));
}

const v8::Value* v8__Object__CallAsConstructor(const v8::Object& self,
                                               const v8::Context& context,
                                               int argc,
                                               const v8::Value* const argv[]) {
  return maybe_local_to_ptr(ptr_to_local(&self)->CallAsConstructor(
      ptr_to_local(&context), argc, const_ptr_array_to_local_array(argv)));
}

const v8::Array* v8__Array__New(v8::Isolate* isolate, int length) {
  return local_to_ptr(v8::Array::New(isolate, length));
}

const v8::Array* v8__Array__New_with_elements(v8::Isolate* isolate,
                                              const v8::Value* const elements[],
                                              size_t length) {
  return local_to_ptr(v8::Array::New(
      isolate, const_ptr_array_to_local_array(elements), length));
}

uint32_t v8__Array__Length(const v8::Array& self) { return self.Length(); }

// The Rust `ArrayIterationResult` enum is declared `#[repr(i32)]` with its
// variants in this order, and is returned directly from the Rust callback, so
// both the discriminants and the width must keep matching V8's. A scoped enum
// with no fixed underlying type is `int` ([dcl.enum]/5); if V8 ever gives
// CallbackResult an explicit underlying type, the size assert catches it even
// though the discriminants would be unchanged.
static_assert(static_cast<int>(v8::Array::CallbackResult::kException) == 0 &&
                  static_cast<int>(v8::Array::CallbackResult::kBreak) == 1 &&
                  static_cast<int>(v8::Array::CallbackResult::kContinue) == 2,
              "Array::CallbackResult discriminant mismatch");
static_assert(sizeof(v8::Array::CallbackResult) == sizeof(int),
              "Array::CallbackResult size mismatch");

// Trampoline that adapts V8's `Local<Value>`-taking iteration callback to a
// plain pointer-based callback that can be implemented in Rust.
namespace {
struct ArrayIterateCallbackData {
  v8::Array::CallbackResult (*callback)(uint32_t index,
                                        const v8::Value* element, void* data);
  void* data;
};

v8::Array::CallbackResult array_iterate_trampoline(uint32_t index,
                                                   v8::Local<v8::Value> element,
                                                   void* data) {
  auto* wrapped = static_cast<ArrayIterateCallbackData*>(data);
  return wrapped->callback(index, local_to_ptr(element), wrapped->data);
}
}  // namespace

// Returns false if iteration was terminated by an exception, true otherwise.
bool v8__Array__Iterate(
    const v8::Array& self, const v8::Context& context,
    v8::Array::CallbackResult (*callback)(uint32_t index,
                                          const v8::Value* element, void* data),
    void* callback_data) {
  ArrayIterateCallbackData wrapped{callback, callback_data};
  return ptr_to_local(&self)
      ->Iterate(ptr_to_local(&context), array_iterate_trampoline, &wrapped)
      .IsJust();
}

const v8::Date* v8__Date__New(const v8::Context& context, double time) {
  // v8::Date::New() is kind of weird in that it returns a v8::Value,
  // not a v8::Date, even though the object is always a Date object.
  // Let's paper over that quirk here.
  v8::MaybeLocal<v8::Date> maybe_date;

  v8::Local<v8::Value> value;
  if (v8::Date::New(ptr_to_local(&context), time).ToLocal(&value)) {
    assert(value->IsDate());
    maybe_date = value.As<v8::Date>();
  }

  return maybe_local_to_ptr(maybe_date);
}

const v8::Date* v8__Date__Parse(const v8::Context& context,
                                const v8::String& date_string) {
  // Like v8::Date::New(), v8::Date::Parse() returns a v8::Value rather than
  // a v8::Date, even though the resulting object is always a Date object.
  v8::MaybeLocal<v8::Date> maybe_date;

  v8::Local<v8::Value> value;
  if (v8::Date::Parse(ptr_to_local(&context), ptr_to_local(&date_string))
          .ToLocal(&value)) {
    assert(value->IsDate());
    maybe_date = value.As<v8::Date>();
  }

  return maybe_local_to_ptr(maybe_date);
}

double v8__Date__ValueOf(const v8::Date& self) { return self.ValueOf(); }

const v8::String* v8__Date__ToISOString(const v8::Date& self) {
  return local_to_ptr(self.ToISOString());
}

const v8::String* v8__Date__ToUTCString(const v8::Date& self) {
  return local_to_ptr(self.ToUTCString());
}

const v8::External* v8__External__New(v8::Isolate* isolate, void* value) {
  return local_to_ptr(
      v8::External::New(isolate, value, v8::kExternalPointerTypeTagDefault));
}

void* v8__External__Value(const v8::External& self) {
  return self.Value(v8::kExternalPointerTypeTagDefault);
}

const v8::Map* v8__Map__New(v8::Isolate* isolate) {
  return local_to_ptr(v8::Map::New(isolate));
}

size_t v8__Map__Size(const v8::Map& self) { return self.Size(); }

void v8__Map__Clear(const v8::Map& self) {
  return ptr_to_local(&self)->Clear();
}

const v8::Value* v8__Map__Get(const v8::Map& self, const v8::Context& context,
                              const v8::Value& key) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Get(ptr_to_local(&context), ptr_to_local(&key)));
}

v8::Map* v8__Map__Set(const v8::Map& self, const v8::Context& context,
                      const v8::Value& key, const v8::Value& value) {
  return maybe_local_to_ptr(ptr_to_local(&self)->Set(
      ptr_to_local(&context), ptr_to_local(&key), ptr_to_local(&value)));
}

MaybeBool v8__Map__Has(const v8::Map& self, const v8::Context& context,
                       const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Has(ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Map__Delete(const v8::Map& self, const v8::Context& context,
                          const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Delete(ptr_to_local(&context), ptr_to_local(&key)));
}

const v8::Array* v8__Map__As__Array(const v8::Map& self) {
  return local_to_ptr(self.AsArray());
}

const v8::Set* v8__Set__New(v8::Isolate* isolate) {
  return local_to_ptr(v8::Set::New(isolate));
}

size_t v8__Set__Size(const v8::Set& self) { return self.Size(); }

void v8__Set__Clear(const v8::Set& self) {
  return ptr_to_local(&self)->Clear();
}

v8::Set* v8__Set__Add(const v8::Set& self, const v8::Context& context,
                      const v8::Value& key) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Add(ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Set__Has(const v8::Set& self, const v8::Context& context,
                       const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Has(ptr_to_local(&context), ptr_to_local(&key)));
}

MaybeBool v8__Set__Delete(const v8::Set& self, const v8::Context& context,
                          const v8::Value& key) {
  return maybe_to_maybe_bool(
      ptr_to_local(&self)->Delete(ptr_to_local(&context), ptr_to_local(&key)));
}

const v8::Array* v8__Set__As__Array(const v8::Set& self) {
  return local_to_ptr(self.AsArray());
}

const v8::Number* v8__Number__New(v8::Isolate* isolate, double value) {
  return *v8::Number::New(isolate, value);
}

double v8__Number__Value(const v8::Number& self) { return self.Value(); }

const v8::Integer* v8__Integer__New(v8::Isolate* isolate, int32_t value) {
  return *v8::Integer::New(isolate, value);
}

const v8::Integer* v8__Integer__NewFromUnsigned(v8::Isolate* isolate,
                                                uint32_t value) {
  return *v8::Integer::NewFromUnsigned(isolate, value);
}

int64_t v8__Integer__Value(const v8::Integer& self) { return self.Value(); }

uint32_t v8__Uint32__Value(const v8::Uint32& self) { return self.Value(); }

int32_t v8__Int32__Value(const v8::Int32& self) { return self.Value(); }

const v8::BigInt* v8__BigInt__New(v8::Isolate* isolate, int64_t value) {
  return local_to_ptr(v8::BigInt::New(isolate, value));
}

const v8::BigInt* v8__BigInt__NewFromUnsigned(v8::Isolate* isolate,
                                              uint64_t value) {
  return local_to_ptr(v8::BigInt::NewFromUnsigned(isolate, value));
}

const v8::BigInt* v8__BigInt__NewFromWords(const v8::Context& context,
                                           int sign_bit, int word_count,
                                           const uint64_t words[]) {
  return maybe_local_to_ptr(v8::BigInt::NewFromWords(
      ptr_to_local(&context), sign_bit, word_count, words));
}

uint64_t v8__BigInt__Uint64Value(const v8::BigInt& self, bool* lossless) {
  return ptr_to_local(&self)->Uint64Value(lossless);
}

int64_t v8__BigInt__Int64Value(const v8::BigInt& self, bool* lossless) {
  return ptr_to_local(&self)->Int64Value(lossless);
}

int v8__BigInt__WordCount(const v8::BigInt& self) {
  return ptr_to_local(&self)->WordCount();
}

void v8__BigInt__ToWordsArray(const v8::BigInt& self, int* sign_bit,
                              int* word_count, uint64_t words[]) {
  ptr_to_local(&self)->ToWordsArray(sign_bit, word_count, words);
}

const v8::ArrayBuffer* v8__ArrayBufferView__Buffer(
    const v8::ArrayBufferView& self) {
  return local_to_ptr(ptr_to_local(&self)->Buffer());
}

const void* v8__ArrayBufferView__Buffer__Data(const v8::ArrayBufferView& self) {
  return ptr_to_local(&self)->Buffer()->Data();
}

size_t v8__ArrayBufferView__ByteLength(const v8::ArrayBufferView& self) {
  return ptr_to_local(&self)->ByteLength();
}

size_t v8__ArrayBufferView__ByteOffset(const v8::ArrayBufferView& self) {
  return ptr_to_local(&self)->ByteOffset();
}

bool v8__ArrayBufferView__HasBuffer(const v8::ArrayBufferView& self) {
  return ptr_to_local(&self)->HasBuffer();
}

size_t v8__ArrayBufferView__CopyContents(const v8::ArrayBufferView& self,
                                         void* dest, int byte_length) {
  return ptr_to_local(&self)->CopyContents(dest, byte_length);
}

memory_span_t v8__ArrayBufferView__GetContents(const v8::ArrayBufferView& self,
                                               memory_span_t rstorage) {
  v8::MemorySpan<uint8_t> storage{rstorage.data, rstorage.size};
  v8::MemorySpan<uint8_t> span = ptr_to_local(&self)->GetContents(storage);
  return {span.data(), span.size()};
}

struct RustAllocatorVtable {
  void* (*allocate)(void* handle, size_t length);
  void* (*allocate_uninitialized)(void* handle, size_t length);
  void (*free)(void* handle, void* data, size_t length);
  void (*drop)(void* handle);
};

class RustAllocator : public v8::ArrayBuffer::Allocator {
 private:
  void* handle;
  const RustAllocatorVtable* vtable;

 public:
  RustAllocator(void* handle, const RustAllocatorVtable* vtable) {
    this->handle = handle;
    this->vtable = vtable;
  }

  RustAllocator(const RustAllocator& that) = delete;
  RustAllocator(RustAllocator&& that) = delete;
  void operator=(const RustAllocator& that) = delete;
  void operator=(RustAllocator&& that) = delete;

  virtual ~RustAllocator() { vtable->drop(handle); }

  void* Allocate(size_t length) final {
    return vtable->allocate(handle, length);
  }

  void* AllocateUninitialized(size_t length) final {
    return vtable->allocate_uninitialized(handle, length);
  }

  void Free(void* data, size_t length) final {
    vtable->free(handle, data, length);
  }
};

v8::ArrayBuffer::Allocator* v8__ArrayBuffer__Allocator__NewDefaultAllocator() {
  return v8::ArrayBuffer::Allocator::NewDefaultAllocator();
}

// Returns nullptr unless V8 was built with pointer compression in multi-cage
// mode, which is the only configuration in which V8 declares the group-scoped
// overload.
v8::ArrayBuffer::Allocator*
v8__ArrayBuffer__Allocator__NewDefaultAllocator__with_group(
    const v8::IsolateGroup& group) {
#if defined(V8_COMPRESS_POINTERS) && \
    !defined(V8_COMPRESS_POINTERS_IN_SHARED_CAGE)
  return v8::ArrayBuffer::Allocator::NewDefaultAllocator(group);
#else
  (void)group;
  return nullptr;
#endif
}

v8::ArrayBuffer::Allocator* v8__ArrayBuffer__Allocator__NewRustAllocator(
    void* handle, const RustAllocatorVtable* vtable) {
  return new RustAllocator(handle, vtable);
}

void v8__ArrayBuffer__Allocator__DELETE(v8::ArrayBuffer::Allocator* self) {
  delete self;
}

const v8::ArrayBuffer* v8__ArrayBuffer__New__with_byte_length(
    v8::Isolate* isolate, size_t byte_length) {
  return local_to_ptr(v8::ArrayBuffer::New(isolate, byte_length));
}

const v8::ArrayBuffer* v8__ArrayBuffer__MaybeNew(
    v8::Isolate* isolate, size_t byte_length,
    v8::BackingStoreInitializationMode initialization_mode) {
  return maybe_local_to_ptr(
      v8::ArrayBuffer::MaybeNew(isolate, byte_length, initialization_mode));
}

const v8::ArrayBuffer* v8__ArrayBuffer__New__with_backing_store(
    v8::Isolate* isolate,
    const std::shared_ptr<v8::BackingStore>& backing_store) {
  return local_to_ptr(v8::ArrayBuffer::New(isolate, backing_store));
}

size_t v8__ArrayBuffer__ByteLength(const v8::ArrayBuffer& self) {
  return self.ByteLength();
}

const v8::DataView* v8__DataView__New(const v8::ArrayBuffer& ab, size_t offset,
                                      size_t length) {
  return local_to_ptr(v8::DataView::New(ptr_to_local(&ab), offset, length));
}

const v8::DataView* v8__DataView__New__with_shared_buffer(
    const v8::SharedArrayBuffer& sab, size_t offset, size_t length) {
  return local_to_ptr(v8::DataView::New(ptr_to_local(&sab), offset, length));
}

struct InternalFieldData {
  uint32_t data;
};

std::vector<InternalFieldData*> deserialized_data;

void DeserializeInternalFields(v8::Local<v8::Object> holder, int index,
                               v8::StartupData payload, void* data) {
  assert(data == nullptr);
  if (payload.raw_size == 0) {
    holder->SetAlignedPointerInInternalField(index, nullptr,
                                             v8::kEmbedderDataTypeTagDefault);
    return;
  }
  InternalFieldData* embedder_field = new InternalFieldData{0};
  memcpy(embedder_field, payload.data, payload.raw_size);
  holder->SetAlignedPointerInInternalField(index, embedder_field,
                                           v8::kEmbedderDataTypeTagDefault);
  deserialized_data.push_back(embedder_field);
}

const v8::Context* v8__Context__New(v8::Isolate* isolate,
                                    const v8::ObjectTemplate* templ,
                                    const v8::Value* global_object,
                                    v8::MicrotaskQueue* microtask_queue) {
  return local_to_ptr(v8::Context::New(
      isolate, nullptr, ptr_to_maybe_local(templ),
      ptr_to_maybe_local(global_object),
      v8::DeserializeInternalFieldsCallback(DeserializeInternalFields, nullptr),
      microtask_queue));
}

bool v8__Context__EQ(const v8::Context& self, const v8::Context& other) {
  return ptr_to_local(&self) == ptr_to_local(&other);
}

void v8__Context__Enter(const v8::Context& self) {
  ptr_to_local(&self)->Enter();
}

void v8__Context__Exit(const v8::Context& self) { ptr_to_local(&self)->Exit(); }

const v8::Object* v8__Context__Global(const v8::Context& self) {
  return local_to_ptr(ptr_to_local(&self)->Global());
}

void v8__Context__DetachGlobal(v8::Context& self) {
  ptr_to_local(&self)->DetachGlobal();
}

uint32_t v8__Context__GetNumberOfEmbedderDataFields(const v8::Context& self) {
  return ptr_to_local(&self)->GetNumberOfEmbedderDataFields();
}

// Embedder data tags cross the FFI boundary as a bare `u16`, and Rust
// hardcodes the default tag as `Context::EMBEDDER_DATA_TAG_DEFAULT = 0` and
// the tag count as `Context::EMBEDDER_DATA_TAG_COUNT`. Out-of-range tags make
// v8::ToExternalPointerTag abort, so Rust range-checks against that constant;
// if V8 raises the count, bump the Rust constant to match.
static_assert(sizeof(v8::EmbedderDataTypeTag) == sizeof(uint16_t),
              "EmbedderDataTypeTag size mismatch");
static_assert(v8::kEmbedderDataTypeTagDefault == 0,
              "kEmbedderDataTypeTagDefault mismatch");
static_assert(V8_EMBEDDER_DATA_TAG_COUNT == 15,
              "V8_EMBEDDER_DATA_TAG_COUNT changed; update "
              "Context::EMBEDDER_DATA_TAG_COUNT in src/context.rs");

void* v8__Context__GetAlignedPointerFromEmbedderData(
    const v8::Context& self, int index, v8::EmbedderDataTypeTag tag) {
  return ptr_to_local(&self)->GetAlignedPointerFromEmbedderData(index, tag);
}

void v8__Context__SetAlignedPointerInEmbedderData(v8::Context& self, int index,
                                                  void* value,
                                                  v8::EmbedderDataTypeTag tag) {
  ptr_to_local(&self)->SetAlignedPointerInEmbedderData(index, value, tag);
}

const v8::Value* v8__Context__GetEmbedderData(const v8::Context& self,
                                              int index) {
  return local_to_ptr(ptr_to_local(&self)->GetEmbedderData(index));
}

void v8__Context__SetEmbedderData(v8::Context& self, int index,
                                  const v8::Value* value) {
  ptr_to_local(&self)->SetEmbedderData(index, ptr_to_local(value));
}

const v8::Data* v8__Context__GetDataFromSnapshotOnce(v8::Context& self,
                                                     size_t index) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->GetDataFromSnapshotOnce<v8::Data>(index));
}

const v8::Object* v8__Context__GetExtrasBindingObject(v8::Context& self) {
  return local_to_ptr(ptr_to_local(&self)->GetExtrasBindingObject());
}

void v8__Context__SetPromiseHooks(v8::Context& self,
                                  const v8::Function* init_hook,
                                  const v8::Function* before_hook,
                                  const v8::Function* after_hook,
                                  const v8::Function* resolve_hook) {
  ptr_to_local(&self)->SetPromiseHooks(
      ptr_to_local(init_hook), ptr_to_local(before_hook),
      ptr_to_local(after_hook), ptr_to_local(resolve_hook));
}

void v8__Context__SetTemporalHostSystemUTCEpochNanosecondsCallback(
    v8::Context& self,
    v8::Context::TemporalHostSystemUTCEpochNanosecondsCallback callback) {
  ptr_to_local(&self)->SetTemporalHostSystemUTCEpochNanosecondsCallback(
      callback);
}

const v8::Value* v8__Context__GetSecurityToken(const v8::Context& self) {
  auto value = ptr_to_local(&self)->GetSecurityToken();
  return local_to_ptr(value);
}

void v8__Context__SetSecurityToken(v8::Context& self, const v8::Value* token) {
  auto c = ptr_to_local(&self);
  c->SetSecurityToken(ptr_to_local(token));
}

void v8__Context__UseDefaultSecurityToken(v8::Context& self) {
  ptr_to_local(&self)->UseDefaultSecurityToken();
}

void v8__Context__AllowCodeGenerationFromStrings(v8::Context& self,
                                                 bool allow) {
  ptr_to_local(&self)->AllowCodeGenerationFromStrings(allow);
}

bool v8__Context_IsCodeGenerationFromStringsAllowed(v8::Context& self) {
  return ptr_to_local(&self)->IsCodeGenerationFromStringsAllowed();
}

v8::MicrotaskQueue* v8__Context__GetMicrotaskQueue(v8::Context& self) {
  return ptr_to_local(&self)->GetMicrotaskQueue();
}

void v8__Context__SetMicrotaskQueue(v8::Context& self,
                                    v8::MicrotaskQueue* microtask_queue) {
  ptr_to_local(&self)->SetMicrotaskQueue(microtask_queue);
}

const v8::Context* v8__Context__FromSnapshot(
    v8::Isolate* isolate, size_t context_snapshot_index,
    v8::Value* global_object, v8::MicrotaskQueue* microtask_queue) {
  v8::MaybeLocal<v8::Context> maybe_local = v8::Context::FromSnapshot(
      isolate, context_snapshot_index,
      v8::DeserializeInternalFieldsCallback(DeserializeInternalFields, nullptr),
      nullptr, ptr_to_maybe_local(global_object), microtask_queue);
  return maybe_local_to_ptr(maybe_local);
}

void v8__Context__SetContinuationPreservedEmbedderData(v8::Isolate* isolate,
                                                       const v8::Value* data) {
  isolate->SetContinuationPreservedEmbedderData(ptr_to_local(data));
}

const v8::Value* v8__Context__GetContinuationPreservedEmbedderData(
    v8::Isolate* isolate) {
  // Since 15.8 this returns Local<Data>. The setter only takes values, so
  // whatever is stored is a Value (undefined by default).
  auto value = isolate->GetContinuationPreservedEmbedderData();
  return local_to_ptr(value.As<v8::Value>());
}

class RustyMicrotaskQueueHandle {
 public:
  RustyMicrotaskQueueHandle(v8::Isolate* isolate, v8::MicrotasksPolicy policy)
      : queue_(v8::MicrotaskQueue::New(isolate, policy)) {}

  v8::MicrotaskQueue* Get() const { return queue_.Get(); }

 private:
  // Microtask queues live on the cppgc heap, so hold them with a Persistent.
  cppgc::Persistent<v8::MicrotaskQueue> queue_;
};

RustyMicrotaskQueueHandle* v8__MicrotaskQueueHandle__New(
    v8::Isolate* isolate, v8::MicrotasksPolicy policy) {
  return new RustyMicrotaskQueueHandle(isolate, policy);
}

void v8__MicrotaskQueueHandle__DELETE(RustyMicrotaskQueueHandle* self) {
  delete self;
}

v8::MicrotaskQueue* v8__MicrotaskQueueHandle__Get(
    const RustyMicrotaskQueueHandle* self) {
  return self->Get();
}

void v8__MicrotaskQueue__PerformCheckpoint(v8::Isolate* isolate,
                                           v8::MicrotaskQueue* self) {
  self->PerformCheckpoint(isolate);
}

// Returns whether `self` is one of `isolate`'s microtask queues. Only the
// queues of `isolate` are looked at, never `self`, which may belong to
// another isolate.
bool v8__MicrotaskQueue__BelongsTo(const v8::MicrotaskQueue* self,
                                   v8::Isolate* isolate) {
  namespace i = v8::internal;
  const i::MicrotaskQueue* queue = static_cast<const i::MicrotaskQueue*>(self);
  i::Isolate* i_isolate = reinterpret_cast<i::Isolate*>(isolate);
  if (queue == i_isolate->default_microtask_queue()) return true;
  for (const auto& weak : i_isolate->microtask_queues()) {
    if (weak.Get() == queue) return true;
  }
  return false;
}

bool v8__MicrotaskQueue__IsRunningMicrotasks(v8::MicrotaskQueue* self) {
  return self->IsRunningMicrotasks();
}

int v8__MicrotaskQueue__GetMicrotasksScopeDepth(v8::MicrotaskQueue* self) {
  return self->GetMicrotasksScopeDepth();
}

void v8__MicrotaskQueue__EnqueueMicrotask(v8::Isolate* isolate,
                                          v8::MicrotaskQueue* self,
                                          v8::Function* callback) {
  self->EnqueueMicrotask(isolate, ptr_to_local(callback));
}

const v8::String* v8__Message__Get(const v8::Message& self) {
  return local_to_ptr(self.Get());
}

const v8::String* v8__Message__GetSourceLine(const v8::Message& self,
                                             const v8::Context& context) {
  return maybe_local_to_ptr(self.GetSourceLine(ptr_to_local(&context)));
}

const v8::Value* v8__Message__GetScriptResourceName(const v8::Message& self) {
  return local_to_ptr(self.GetScriptResourceName());
}

int v8__Message__GetLineNumber(const v8::Message& self,
                               const v8::Context& context) {
  v8::Maybe<int> maybe = self.GetLineNumber(ptr_to_local(&context));
  if (maybe.IsJust()) {
    return maybe.ToChecked();
  } else {
    return -1;
  }
}

const v8::StackTrace* v8__Message__GetStackTrace(const v8::Message& self) {
  return local_to_ptr(self.GetStackTrace());
}

int v8__Message__GetStartPosition(const v8::Message& self) {
  return self.GetStartPosition();
}

int v8__Message__GetEndPosition(const v8::Message& self) {
  return self.GetEndPosition();
}

int v8__Message__GetWasmFunctionIndex(const v8::Message& self) {
  return self.GetWasmFunctionIndex();
}

int v8__Message__ErrorLevel(const v8::Message& self) {
  return self.ErrorLevel();
}

int v8__Message__GetStartColumn(const v8::Message& self) {
  return self.GetStartColumn();
}

int v8__Message__GetEndColumn(const v8::Message& self) {
  return self.GetEndColumn();
}

bool v8__Message__IsSharedCrossOrigin(const v8::Message& self) {
  return self.IsSharedCrossOrigin();
}

bool v8__Message__IsOpaque(const v8::Message& self) { return self.IsOpaque(); }

const v8::Value* v8__Exception__RangeError(const v8::String& message) {
  return local_to_ptr(v8::Exception::RangeError(ptr_to_local(&message)));
}

const v8::Value* v8__Exception__ReferenceError(const v8::String& message) {
  return local_to_ptr(v8::Exception::ReferenceError(ptr_to_local(&message)));
}

const v8::Value* v8__Exception__SyntaxError(const v8::String& message) {
  return local_to_ptr(v8::Exception::SyntaxError(ptr_to_local(&message)));
}

const v8::Value* v8__Exception__TypeError(const v8::String& message) {
  return local_to_ptr(v8::Exception::TypeError(ptr_to_local(&message)));
}

const v8::Value* v8__Exception__Error(const v8::String& message) {
  return local_to_ptr(v8::Exception::Error(ptr_to_local(&message)));
}

const v8::Message* v8__Exception__CreateMessage(v8::Isolate* isolate,
                                                const v8::Value& exception) {
  return local_to_ptr(
      v8::Exception::CreateMessage(isolate, ptr_to_local(&exception)));
}

const v8::StackTrace* v8__Exception__GetStackTrace(const v8::Value& exception) {
  return local_to_ptr(v8::Exception::GetStackTrace(ptr_to_local(&exception)));
}

MaybeBool v8__Exception__CaptureStackTrace(const v8::Context& context,
                                           const v8::Object& object) {
  return maybe_to_maybe_bool(v8::Exception::CaptureStackTrace(
      ptr_to_local(&context), ptr_to_local(&object)));
}

const v8::Function* v8__Function__New(
    const v8::Context& context, v8::FunctionCallback callback,
    const v8::Value* data_or_null, int length,
    v8::ConstructorBehavior constructor_behavior,
    v8::SideEffectType side_effect_type) {
  return maybe_local_to_ptr(v8::Function::New(
      ptr_to_local(&context), callback, ptr_to_local(data_or_null), length,
      constructor_behavior, side_effect_type));
}

const v8::Value* v8__Function__Call(const v8::Function& self,
                                    const v8::Context& context,
                                    const v8::Value& recv, int argc,
                                    const v8::Value* const argv[]) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Call(ptr_to_local(&context), ptr_to_local(&recv),
                                argc, const_ptr_array_to_local_array(argv)));
}

const v8::Object* v8__Function__NewInstance(const v8::Function& self,
                                            const v8::Context& context,
                                            int argc,
                                            const v8::Value* const argv[]) {
  return maybe_local_to_ptr(ptr_to_local(&self)->NewInstance(
      ptr_to_local(&context), argc, const_ptr_array_to_local_array(argv)));
}

const v8::Value* v8__Function__GetName(const v8::Function& self) {
  return local_to_ptr(self.GetName());
}

void v8__Function__SetName(const v8::Function& self, const v8::String& name) {
  return ptr_to_local(&self)->SetName(ptr_to_local(&name));
}

int v8__Function__GetScriptColumnNumber(const v8::Function& self) {
  return ptr_to_local(&self)->GetScriptColumnNumber();
}

int v8__Function__GetScriptLineNumber(const v8::Function& self) {
  return ptr_to_local(&self)->GetScriptLineNumber();
}

int v8__Function__ScriptId(const v8::Function& self) {
  return ptr_to_local(&self)->ScriptId();
}

void v8__Function__GetScriptOrigin(const v8::Function& self,
                                   uninit_t<v8::ScriptOrigin>* out) {
  construct_in_place<v8::ScriptOrigin>(out,
                                       ptr_to_local(&self)->GetScriptOrigin());
}

const v8::Signature* v8__Signature__New(v8::Isolate* isolate,
                                        const v8::FunctionTemplate* templ) {
  return local_to_ptr(v8::Signature::New(isolate, ptr_to_local(templ)));
}

const v8::FunctionTemplate* v8__FunctionTemplate__New(
    v8::Isolate* isolate, v8::FunctionCallback callback,
    const v8::Value* data_or_null, const v8::Signature* signature_or_null,
    int length, v8::ConstructorBehavior constructor_behavior,
    v8::SideEffectType side_effect_type, const v8::CFunction* c_functions,
    size_t c_functions_len) {
  v8::MemorySpan<const v8::CFunction> overloads{c_functions, c_functions_len};
  return local_to_ptr(v8::FunctionTemplate::NewWithCFunctionOverloads(
      isolate, callback, ptr_to_local(data_or_null),
      ptr_to_local(signature_or_null), length, constructor_behavior,
      side_effect_type, overloads));
}

const v8::Function* v8__FunctionTemplate__GetFunction(
    const v8::FunctionTemplate& self, const v8::Context& context) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->GetFunction(ptr_to_local(&context)));
}

void v8__FunctionTemplate__SetClassName(const v8::FunctionTemplate& self,
                                        const v8::String& name) {
  ptr_to_local(&self)->SetClassName(ptr_to_local(&name));
}

void v8__FunctionTemplate__SetAccessorProperty(const v8::FunctionTemplate& self,
                                               const v8::Name& key,
                                               v8::FunctionTemplate& getter,
                                               v8::FunctionTemplate& setter,
                                               v8::PropertyAttribute attr) {
  ptr_to_local(&self)->SetAccessorProperty(
      ptr_to_local(&key), ptr_to_local(&getter), ptr_to_local(&setter), attr);
}

void v8__FunctionTemplate__Inherit(const v8::FunctionTemplate& self,
                                   const v8::FunctionTemplate& parent) {
  ptr_to_local(&self)->Inherit(ptr_to_local(&parent));
}

void v8__FunctionTemplate__ReadOnlyPrototype(const v8::FunctionTemplate& self) {
  ptr_to_local(&self)->ReadOnlyPrototype();
}

void v8__FunctionTemplate__RemovePrototype(const v8::FunctionTemplate& self) {
  ptr_to_local(&self)->RemovePrototype();
}

bool v8__FunctionTemplate__HasInstance(const v8::FunctionTemplate& self,
                                       const v8::Value& object) {
  return ptr_to_local(&self)->HasInstance(ptr_to_local(&object));
}

const v8::DictionaryTemplate* v8__DictionaryTemplate__New(
    v8::Isolate* isolate, const char* const* names, const size_t* name_lengths,
    size_t names_len) {
  std::vector<std::string_view> vec;
  vec.reserve(names_len);
  for (size_t i = 0; i < names_len; i++) {
    vec.emplace_back(names[i], name_lengths[i]);
  }
  return local_to_ptr(v8::DictionaryTemplate::New(
      isolate, std::span<const std::string_view>(vec.data(), vec.size())));
}

const v8::Object* v8__DictionaryTemplate__NewInstance(
    const v8::DictionaryTemplate& self, const v8::Context& context,
    const v8::Value* const* values, size_t values_len) {
  std::vector<v8::MaybeLocal<v8::Value>> vec;
  vec.reserve(values_len);
  for (size_t i = 0; i < values_len; i++) {
    if (values[i] == nullptr) {
      vec.emplace_back();
    } else {
      vec.emplace_back(ptr_to_local(values[i]));
    }
  }
  return local_to_ptr(ptr_to_local(&self)->NewInstance(
      ptr_to_local(&context),
      std::span<v8::MaybeLocal<v8::Value>>(vec.data(), vec.size())));
}

const v8::ObjectTemplate* v8__FunctionTemplate__PrototypeTemplate(
    const v8::FunctionTemplate& self) {
  return local_to_ptr(ptr_to_local(&self)->PrototypeTemplate());
}

const v8::ObjectTemplate* v8__FunctionTemplate__InstanceTemplate(
    const v8::FunctionTemplate& self) {
  return local_to_ptr(ptr_to_local(&self)->InstanceTemplate());
}

v8::Isolate* v8__FunctionCallbackInfo__GetIsolate(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return self.GetIsolate();
}

function_callback_info_parts_t v8__FunctionCallbackInfo__GetParts(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  v8::ReturnValue<v8::Value> rv = self.GetReturnValue();
  return {
      self.GetIsolate(),
      *reinterpret_cast<void**>(&rv),
      local_to_ptr(self.Data()),
      self.Length(),
  };
}

const v8::Value* v8__FunctionCallbackInfo__Data(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return local_to_ptr(self.Data());
}

const v8::Value* v8__FunctionCallbackInfo__NewTarget(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return local_to_ptr(self.NewTarget());
}

const v8::Object* v8__FunctionCallbackInfo__This(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return local_to_ptr(self.This());
}

const v8::Value* v8__FunctionCallbackInfo__Get(
    const v8::FunctionCallbackInfo<v8::Value>& self, int index) {
  return local_to_ptr(self[index]);
}

int v8__FunctionCallbackInfo__Length(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return self.Length();
}

bool v8__FunctionCallbackInfo__IsConstructCall(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  return self.IsConstructCall();
}

uintptr_t* v8__FunctionCallbackInfo__GetReturnValue(
    const v8::FunctionCallbackInfo<v8::Value>& self) {
  v8::ReturnValue<v8::Value> rv = self.GetReturnValue();
  return *reinterpret_cast<uintptr_t**>(&rv);
}

v8::Isolate* v8__PropertyCallbackInfo__GetIsolate(
    const v8::PropertyCallbackInfo<v8::Value>& self) {
  return self.GetIsolate();
}

const v8::Value* v8__PropertyCallbackInfo__Data(
    const v8::PropertyCallbackInfo<v8::Value>& self) {
  return local_to_ptr(self.Data());
}

const v8::Object* v8__PropertyCallbackInfo__Holder(
    const v8::PropertyCallbackInfo<v8::Value>& self) {
  return local_to_ptr(self.Holder());
}

uintptr_t* v8__PropertyCallbackInfo__GetReturnValue(
    const v8::PropertyCallbackInfo<v8::Value>& self) {
  v8::ReturnValue<v8::Value> rv = self.GetReturnValue();
  return *reinterpret_cast<uintptr_t**>(&rv);
}

bool v8__PropertyCallbackInfo__ShouldThrowOnError(
    const v8::PropertyCallbackInfo<v8::Value>& self) {
  return self.ShouldThrowOnError();
}

const v8::RegExp* v8__RegExp__New(const v8::Context& context,
                                  const v8::String& pattern,
                                  v8::RegExp::Flags flags) {
  return maybe_local_to_ptr(
      v8::RegExp::New(ptr_to_local(&context), ptr_to_local(&pattern), flags));
}

const v8::Object* v8__RegExp__Exec(v8::RegExp& self, const v8::Context& context,
                                   const v8::String& subject) {
  return maybe_local_to_ptr(
      self.Exec(ptr_to_local(&context), ptr_to_local(&subject)));
}

// The largest value v8::RegExp::NewWithBacktrackLimit accepts. It ApiChecks
// (i.e. aborts, even in release builds) that the limit is a valid Smi, and the
// Smi range depends on whether pointer compression is enabled, so report it to
// Rust rather than hardcoding a bound there.
uint32_t v8__RegExp__MAX_BACKTRACK_LIMIT() {
  return static_cast<uint32_t>(v8::internal::kSmiMaxValue);
}

const v8::RegExp* v8__RegExp__NewWithBacktrackLimit(const v8::Context& context,
                                                    const v8::String& pattern,
                                                    v8::RegExp::Flags flags,
                                                    uint32_t backtrack_limit) {
  return maybe_local_to_ptr(v8::RegExp::NewWithBacktrackLimit(
      ptr_to_local(&context), ptr_to_local(&pattern), flags, backtrack_limit));
}

const v8::String* v8__RegExp__GetSource(const v8::RegExp& self) {
  return local_to_ptr(self.GetSource());
}

void v8__ReturnValue__Value__Set(v8::ReturnValue<v8::Value>* self,
                                 const v8::Value& value) {
  self->Set(ptr_to_local(&value));
}

void v8__ReturnValue__Value__Set__Bool(v8::ReturnValue<v8::Value>* self,
                                       bool i) {
  self->Set(i);
}

void v8__ReturnValue__Value__Set__Int32(v8::ReturnValue<v8::Value>* self,
                                        int32_t i) {
  self->Set(i);
}

void v8__ReturnValue__Value__Set__Uint32(v8::ReturnValue<v8::Value>* self,
                                         uint32_t i) {
  self->Set(i);
}

void v8__ReturnValue__Value__Set__Double(v8::ReturnValue<v8::Value>* self,
                                         double i) {
  self->Set(i);
}

void v8__ReturnValue__Value__SetNull(v8::ReturnValue<v8::Value>* self) {
  self->SetNull();
}

void v8__ReturnValue__Value__SetUndefined(v8::ReturnValue<v8::Value>* self) {
  self->SetUndefined();
}

void v8__ReturnValue__Value__SetEmptyString(v8::ReturnValue<v8::Value>* self) {
  self->SetEmptyString();
}

const v8::Value* v8__ReturnValue__Value__Get(
    const v8::ReturnValue<v8::Value>& self) {
  return local_to_ptr(self.Get());
}

// Note: StackTraceOptions is deprecated, kDetailed is always used
const v8::StackTrace* v8__StackTrace__CurrentStackTrace(v8::Isolate* isolate,
                                                        int frame_limit) {
  return local_to_ptr(v8::StackTrace::CurrentStackTrace(isolate, frame_limit));
}

const v8::String* v8__StackTrace__CurrentScriptNameOrSourceURL(
    v8::Isolate* isolate) {
  return local_to_ptr(v8::StackTrace::CurrentScriptNameOrSourceURL(isolate));
}

int v8__StackTrace__GetFrameCount(const v8::StackTrace& self) {
  return self.GetFrameCount();
}

const v8::StackFrame* v8__StackTrace__GetFrame(const v8::StackTrace& self,
                                               v8::Isolate* isolate,
                                               uint32_t index) {
  return local_to_ptr(self.GetFrame(isolate, index));
}

int v8__StackFrame__GetLineNumber(const v8::StackFrame& self) {
  return self.GetLineNumber();
}

int v8__StackFrame__GetColumn(const v8::StackFrame& self) {
  return self.GetColumn();
}

int v8__StackFrame__GetScriptId(const v8::StackFrame& self) {
  return self.GetScriptId();
}

const v8::String* v8__StackFrame__GetScriptName(const v8::StackFrame& self) {
  return local_to_ptr(self.GetScriptName());
}

const v8::String* v8__StackFrame__GetScriptNameOrSourceURL(
    const v8::StackFrame& self) {
  return local_to_ptr(self.GetScriptNameOrSourceURL());
}

const v8::String* v8__StackFrame__GetScriptSource(const v8::StackFrame& self) {
  return local_to_ptr(self.GetScriptSource());
}

const v8::String* v8__StackFrame__GetScriptSourceMappingURL(
    const v8::StackFrame& self) {
  return local_to_ptr(self.GetScriptSourceMappingURL());
}

const v8::String* v8__StackFrame__GetFunctionName(const v8::StackFrame& self) {
  return local_to_ptr(self.GetFunctionName());
}

bool v8__StackFrame__IsEval(const v8::StackFrame& self) {
  return self.IsEval();
}

bool v8__StackFrame__IsConstructor(const v8::StackFrame& self) {
  return self.IsConstructor();
}

bool v8__StackFrame__IsWasm(const v8::StackFrame& self) {
  return self.IsWasm();
}

bool v8__StackFrame__IsUserJavaScript(const v8::StackFrame& self) {
  return self.IsUserJavaScript();
}

void v8__TryCatch__CONSTRUCT(uninit_t<v8::TryCatch>* buf,
                             v8::Isolate* isolate) {
  construct_in_place<v8::TryCatch>(buf, isolate);
}

void v8__TryCatch__DESTRUCT(v8::TryCatch* self) { self->~TryCatch(); }

bool v8__TryCatch__HasCaught(const v8::TryCatch& self) {
  return self.HasCaught();
}

bool v8__TryCatch__CanContinue(const v8::TryCatch& self) {
  return self.CanContinue();
}

bool v8__TryCatch__HasTerminated(const v8::TryCatch& self) {
  return self.HasTerminated();
}

const v8::Value* v8__TryCatch__Exception(const v8::TryCatch& self) {
  return local_to_ptr(self.Exception());
}

const v8::Value* v8__TryCatch__StackTrace(const v8::TryCatch& self,
                                          const v8::Context& context) {
  return maybe_local_to_ptr(self.StackTrace(ptr_to_local(&context)));
}

const v8::Message* v8__TryCatch__Message(const v8::TryCatch& self) {
  return local_to_ptr(self.Message());
}

void v8__TryCatch__Reset(v8::TryCatch* self) { self->Reset(); }

const v8::Value* v8__TryCatch__ReThrow(v8::TryCatch* self) {
  return local_to_ptr(self->ReThrow());
}

bool v8__TryCatch__IsVerbose(const v8::TryCatch& self) {
  return self.IsVerbose();
}

void v8__TryCatch__SetVerbose(v8::TryCatch* self, bool value) {
  self->SetVerbose(value);
}

void v8__TryCatch__SetCaptureMessage(v8::TryCatch* self, bool value) {
  self->SetCaptureMessage(value);
}

void v8__DisallowJavascriptExecutionScope__CONSTRUCT(
    uninit_t<v8::Isolate::DisallowJavascriptExecutionScope>* buf,
    v8::Isolate* isolate,
    v8::Isolate::DisallowJavascriptExecutionScope::OnFailure on_failure) {
  construct_in_place<v8::Isolate::DisallowJavascriptExecutionScope>(
      buf, isolate, on_failure);
}

void v8__DisallowJavascriptExecutionScope__DESTRUCT(
    v8::Isolate::DisallowJavascriptExecutionScope* self) {
  self->~DisallowJavascriptExecutionScope();
}

void v8__AllowJavascriptExecutionScope__CONSTRUCT(
    uninit_t<v8::Isolate::AllowJavascriptExecutionScope>* buf,
    v8::Isolate* isolate) {
  construct_in_place<v8::Isolate::AllowJavascriptExecutionScope>(buf, isolate);
}

void v8__AllowJavascriptExecutionScope__DESTRUCT(
    v8::Isolate::AllowJavascriptExecutionScope* self) {
  self->~AllowJavascriptExecutionScope();
}

#define V(NAME)                                                          \
  const v8::NAME* v8__##NAME##__New(const v8::ArrayBuffer& buf_ptr,      \
                                    size_t byte_offset, size_t length) { \
    return local_to_ptr(                                                 \
        v8::NAME::New(ptr_to_local(&buf_ptr), byte_offset, length));     \
  }
EACH_TYPED_ARRAY(V)
#undef V

#define V(NAME)                                                      \
  const v8::NAME* v8__##NAME##__New__with_shared_buffer(             \
      const v8::SharedArrayBuffer& buf_ptr, size_t byte_offset,      \
      size_t length) {                                               \
    return local_to_ptr(                                             \
        v8::NAME::New(ptr_to_local(&buf_ptr), byte_offset, length)); \
  }
EACH_TYPED_ARRAY(V)
#undef V

const v8::Script* v8__Script__Compile(const v8::Context& context,
                                      const v8::String& source,
                                      const v8::ScriptOrigin& origin) {
  return maybe_local_to_ptr(
      v8::Script::Compile(ptr_to_local(&context), ptr_to_local(&source),
                          const_cast<v8::ScriptOrigin*>(&origin)));
}

const v8::UnboundScript* v8__Script__GetUnboundScript(
    const v8::Script& script) {
  return local_to_ptr(ptr_to_local(&script)->GetUnboundScript());
}

int v8__Script__ScriptId(const v8::Script& script) {
  return ptr_to_local(&script)->ScriptId();
}

const v8::Script* v8__UnboundScript__BindToCurrentContext(
    const v8::UnboundScript& unbound_script) {
  return local_to_ptr(ptr_to_local(&unbound_script)->BindToCurrentContext());
}

int v8__UnboundScript__ScriptId(const v8::UnboundScript& unbound_script) {
  return ptr_to_local(&unbound_script)->ScriptId();
}

v8::ScriptCompiler::CachedData* v8__UnboundScript__CreateCodeCache(
    const v8::UnboundScript& unbound_script) {
  return v8::ScriptCompiler::CreateCodeCache(ptr_to_local(&unbound_script));
}

v8::Value* v8__UnboundScript__GetSourceMappingURL(
    const v8::UnboundScript& unbound_script) {
  return local_to_ptr(ptr_to_local(&unbound_script)->GetSourceMappingURL());
}

v8::Value* v8__UnboundScript__GetSourceURL(
    const v8::UnboundScript& unbound_script) {
  return local_to_ptr(ptr_to_local(&unbound_script)->GetSourceURL());
}

v8::ScriptCompiler::CachedData* v8__UnboundModuleScript__CreateCodeCache(
    const v8::UnboundModuleScript& unbound_module_script) {
  return v8::ScriptCompiler::CreateCodeCache(
      ptr_to_local(&unbound_module_script));
}

v8::Value* v8__UnboundModuleScript__GetSourceMappingURL(
    const v8::UnboundModuleScript& unbound_module_script) {
  return local_to_ptr(
      ptr_to_local(&unbound_module_script)->GetSourceMappingURL());
}

v8::Value* v8__UnboundModuleScript__GetSourceURL(
    const v8::UnboundModuleScript& unbound_module_script) {
  return local_to_ptr(ptr_to_local(&unbound_module_script)->GetSourceURL());
}

v8::ScriptCompiler::CachedData* v8__Function__CreateCodeCache(
    const v8::Function& self) {
  return v8::ScriptCompiler::CreateCodeCacheForFunction(ptr_to_local(&self));
}

const v8::Value* v8__Script__Run(const v8::Script& script,
                                 const v8::Context& context) {
  return maybe_local_to_ptr(ptr_to_local(&script)->Run(ptr_to_local(&context)));
}

void v8__ScriptOrigin__CONSTRUCT(
    uninit_t<v8::ScriptOrigin>* buf, const v8::Value& resource_name,
    int resource_line_offset, int resource_column_offset,
    bool resource_is_shared_cross_origin, int script_id,
    const v8::Value* source_map_url, bool resource_is_opaque, bool is_wasm,
    bool is_module, const v8::Data* host_defined_options) {
  construct_in_place<v8::ScriptOrigin>(
      buf, ptr_to_local(&resource_name), resource_line_offset,
      resource_column_offset, resource_is_shared_cross_origin, script_id,
      ptr_to_local(source_map_url), resource_is_opaque, is_wasm, is_module,
      ptr_to_local(host_defined_options));
}

int v8__ScriptOrigin__ScriptId(const v8::ScriptOrigin& self) {
  return self.ScriptId();
}

const v8::Value* v8__ScriptOrigin__ResourceName(const v8::ScriptOrigin& self) {
  return local_to_ptr(self.ResourceName());
}

const v8::Value* v8__ScriptOrigin__SourceMapUrl(const v8::ScriptOrigin& self) {
  return local_to_ptr(self.SourceMapUrl());
}

const v8::Value* v8__ScriptOrModule__GetResourceName(
    const v8::ScriptOrModule& self) {
  return local_to_ptr(ptr_to_local(&self)->GetResourceName());
}

const v8::Data* v8__ScriptOrModule__HostDefinedOptions(
    const v8::ScriptOrModule& self) {
  return local_to_ptr(ptr_to_local(&self)->HostDefinedOptions());
}

const v8::SharedArrayBuffer* v8__SharedArrayBuffer__New__with_byte_length(
    v8::Isolate* isolate, size_t byte_length) {
  return local_to_ptr(v8::SharedArrayBuffer::New(isolate, byte_length));
}

const v8::SharedArrayBuffer* v8__SharedArrayBuffer__New__with_backing_store(
    v8::Isolate* isolate,
    const std::shared_ptr<v8::BackingStore>& backing_store) {
  return local_to_ptr(v8::SharedArrayBuffer::New(isolate, backing_store));
}

size_t v8__SharedArrayBuffer__ByteLength(const v8::SharedArrayBuffer& self) {
  return self.ByteLength();
}

two_pointers_t v8__SharedArrayBuffer__GetBackingStore(
    const v8::SharedArrayBuffer& self) {
  return make_pod<two_pointers_t>(ptr_to_local(&self)->GetBackingStore());
}

v8::BackingStore* v8__SharedArrayBuffer__NewBackingStore__with_byte_length(
    v8::Isolate* isolate, size_t byte_length) {
  std::unique_ptr<v8::BackingStore> u =
      v8::SharedArrayBuffer::NewBackingStore(isolate, byte_length);
  return u.release();
}

v8::BackingStore* v8__SharedArrayBuffer__NewBackingStore__with_mode(
    v8::Isolate* isolate, size_t byte_length,
    v8::BackingStoreInitializationMode initialization_mode,
    v8::BackingStoreOnFailureMode on_failure) {
  std::unique_ptr<v8::BackingStore> u = v8::SharedArrayBuffer::NewBackingStore(
      isolate, byte_length, initialization_mode, on_failure);
  return u.release();
}

void* v8__SharedArrayBuffer__Data(const v8::SharedArrayBuffer& self) {
  return ptr_to_local(&self)->Data();
}

v8::BackingStore* v8__SharedArrayBuffer__NewBackingStore__with_data(
    void* data, size_t byte_length, v8::BackingStore::DeleterCallback deleter,
    void* deleter_data) {
  std::unique_ptr<v8::BackingStore> u = v8::SharedArrayBuffer::NewBackingStore(
      data, byte_length, deleter, deleter_data);
  return u.release();
}

const v8::Value* v8__JSON__Parse(const v8::Context& context,
                                 const v8::String& json_string) {
  return maybe_local_to_ptr(
      v8::JSON::Parse(ptr_to_local(&context), ptr_to_local(&json_string)));
}

const v8::String* v8__JSON__Stringify(const v8::Context& context,
                                      const v8::Value& json_object) {
  return maybe_local_to_ptr(
      v8::JSON::Stringify(ptr_to_local(&context), ptr_to_local(&json_object)));
}

const v8::Promise::Resolver* v8__Promise__Resolver__New(
    const v8::Context& context) {
  return maybe_local_to_ptr(v8::Promise::Resolver::New(ptr_to_local(&context)));
}

const v8::Promise* v8__Promise__Resolver__GetPromise(
    const v8::Promise::Resolver& self) {
  return local_to_ptr(ptr_to_local(&self)->GetPromise());
}

MaybeBool v8__Promise__Resolver__Resolve(const v8::Promise::Resolver& self,
                                         const v8::Context& context,
                                         const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->Resolve(
      ptr_to_local(&context), ptr_to_local(&value)));
}

MaybeBool v8__Promise__Resolver__Reject(const v8::Promise::Resolver& self,
                                        const v8::Context& context,
                                        const v8::Value& value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->Reject(ptr_to_local(&context),
                                                         ptr_to_local(&value)));
}

v8::Promise::PromiseState v8__Promise__State(const v8::Promise& self) {
  return ptr_to_local(&self)->State();
}

bool v8__Promise__HasHandler(const v8::Promise& self) {
  return ptr_to_local(&self)->HasHandler();
}

void v8__Promise__MarkAsHandled(const v8::Promise& self) {
  ptr_to_local(&self)->MarkAsHandled();
}

const v8::Value* v8__Promise__Result(const v8::Promise& self) {
  return local_to_ptr(ptr_to_local(&self)->Result());
}

const v8::Promise* v8__Promise__Catch(const v8::Promise& self,
                                      const v8::Context& context,
                                      const v8::Function& handler) {
  return maybe_local_to_ptr(ptr_to_local(&self)->Catch(ptr_to_local(&context),
                                                       ptr_to_local(&handler)));
}

const v8::Promise* v8__Promise__Then(const v8::Promise& self,
                                     const v8::Context& context,
                                     const v8::Function& handler) {
  return maybe_local_to_ptr(ptr_to_local(&self)->Then(ptr_to_local(&context),
                                                      ptr_to_local(&handler)));
}

const v8::Promise* v8__Promise__Then2(const v8::Promise& self,
                                      const v8::Context& context,
                                      const v8::Function& on_fulfilled,
                                      const v8::Function& on_rejected) {
  return maybe_local_to_ptr(ptr_to_local(&self)->Then(
      ptr_to_local(&context), ptr_to_local(&on_fulfilled),
      ptr_to_local(&on_rejected)));
}

v8::PromiseRejectEvent v8__PromiseRejectMessage__GetEvent(
    const v8::PromiseRejectMessage& self) {
  return self.GetEvent();
}

const v8::Promise* v8__PromiseRejectMessage__GetPromise(
    const v8::PromiseRejectMessage& self) {
  return local_to_ptr(self.GetPromise());
}

const v8::Value* v8__PromiseRejectMessage__GetValue(
    const v8::PromiseRejectMessage& self) {
  return local_to_ptr(self.GetValue());
}

const v8::Proxy* v8__Proxy__New(const v8::Context& context,
                                const v8::Object& target,
                                const v8::Object& handler) {
  return maybe_local_to_ptr(v8::Proxy::New(
      ptr_to_local(&context), ptr_to_local(&target), ptr_to_local(&handler)));
}

const v8::Value* v8__Proxy__GetHandler(const v8::Proxy& self) {
  return local_to_ptr(ptr_to_local(&self)->GetHandler());
}

const v8::Value* v8__Proxy__GetTarget(const v8::Proxy& self) {
  return local_to_ptr(ptr_to_local(&self)->GetTarget());
}

bool v8__Proxy__IsRevoked(const v8::Proxy& self) {
  return ptr_to_local(&self)->IsRevoked();
}

void v8__Proxy__Revoke(const v8::Proxy& self) { ptr_to_local(&self)->Revoke(); }

void v8__SnapshotCreator__CONSTRUCT(uninit_t<v8::SnapshotCreator>* buf,
                                    const v8::Isolate::CreateParams& params) {
  construct_in_place<v8::SnapshotCreator>(buf, params);
}

void v8__SnapshotCreator__DESTRUCT(v8::SnapshotCreator* self) {
  self->~SnapshotCreator();
}

bool v8__StartupData__CanBeRehashed(const v8::StartupData& self) {
  return self.CanBeRehashed();
}

bool v8__StartupData__IsValid(const v8::StartupData& self) {
  return self.IsValid();
}

void v8__StartupData__data__DELETE(const char* data) { delete[] data; }

v8::Isolate* v8__SnapshotCreator__GetIsolate(const v8::SnapshotCreator& self) {
  // `v8::SnapshotCreator::GetIsolate()` is not declared as a const method, but
  // this appears to be a mistake.
  auto self_ptr = const_cast<v8::SnapshotCreator*>(&self);
  return self_ptr->GetIsolate();
}

v8::StartupData SerializeInternalFields(v8::Local<v8::Object> holder, int index,
                                        void* data) {
  assert(data == nullptr);
  InternalFieldData* embedder_field = static_cast<InternalFieldData*>(
      holder->GetAlignedPointerFromInternalField(
          index, v8::kEmbedderDataTypeTagDefault));
  if (embedder_field == nullptr) return {nullptr, 0};
  int size = sizeof(*embedder_field);
  char* payload = new char[size];
  // We simply use memcpy to serialize the content.
  memcpy(payload, embedder_field, size);
  return {payload, size};
}

void v8__SnapshotCreator__SetDefaultContext(v8::SnapshotCreator* self,
                                            const v8::Context& context) {
  self->SetDefaultContext(ptr_to_local(&context), SerializeInternalFields);
}

size_t v8__SnapshotCreator__AddContext(v8::SnapshotCreator* self,
                                       const v8::Context& context) {
  return self->AddContext(ptr_to_local(&context), SerializeInternalFields);
}

size_t v8__SnapshotCreator__AddData_to_isolate(v8::SnapshotCreator* self,
                                               const v8::Data& data) {
  return self->AddData(ptr_to_local(&data));
}

size_t v8__SnapshotCreator__AddData_to_context(v8::SnapshotCreator* self,
                                               const v8::Context& context,
                                               const v8::Data& data) {
  return self->AddData(ptr_to_local(&context), ptr_to_local(&data));
}

v8::StartupData v8__SnapshotCreator__CreateBlob(
    v8::SnapshotCreator* self,
    v8::SnapshotCreator::FunctionCodeHandling function_code_handling) {
  return self->CreateBlob(function_code_handling);
}

// Rust-side callbacks for trait-based CustomPlatform (PlatformImpl trait).
// Each callback corresponds to a C++ virtual method on TaskRunner or Platform.
// `context` is a pointer to the Rust Box<dyn PlatformImpl>.
// Task ownership is transferred to Rust — Rust is responsible for calling
// Run() and deleting the task.
extern "C" {
void v8__Platform__CustomPlatform__BASE__PostTask(void* context, void* isolate,
                                                  v8::Task* task);
void v8__Platform__CustomPlatform__BASE__PostNonNestableTask(void* context,
                                                             void* isolate,
                                                             v8::Task* task);
void v8__Platform__CustomPlatform__BASE__PostDelayedTask(
    void* context, void* isolate, v8::Task* task, double delay_in_seconds);
void v8__Platform__CustomPlatform__BASE__PostNonNestableDelayedTask(
    void* context, void* isolate, v8::Task* task, double delay_in_seconds);
void v8__Platform__CustomPlatform__BASE__PostIdleTask(void* context,
                                                      void* isolate,
                                                      v8::IdleTask* task);
void v8__Platform__CustomPlatform__BASE__DROP(void* context);
}

// FFI functions for running and deleting V8 tasks from Rust.
extern "C" {
void v8__Task__Run(v8::Task* task) { task->Run(); }
void v8__Task__DELETE(v8::Task* task) { delete task; }
void v8__IdleTask__Run(v8::IdleTask* task, double deadline_in_seconds) {
  task->Run(deadline_in_seconds);
}
void v8__IdleTask__DELETE(v8::IdleTask* task) { delete task; }
}

// TaskRunner wrapper that intercepts all PostTask* virtual methods and
// transfers task ownership to Rust via the PlatformImpl trait. The Rust
// side is responsible for scheduling and calling task->Run().
//
// The wrapped runner is kept for capability queries (IdleTasksEnabled, etc.)
// but tasks are NOT forwarded to it — Rust owns them entirely.
class CustomTaskRunner final : public v8::TaskRunner {
 public:
  CustomTaskRunner(std::shared_ptr<v8::TaskRunner> wrapped, void* context,
                   v8::Isolate* isolate)
      : wrapped_(std::move(wrapped)), context_(context), isolate_(isolate) {}

  bool IdleTasksEnabled() override { return wrapped_->IdleTasksEnabled(); }
  bool NonNestableTasksEnabled() const override {
    return wrapped_->NonNestableTasksEnabled();
  }
  bool NonNestableDelayedTasksEnabled() const override {
    return wrapped_->NonNestableDelayedTasksEnabled();
  }

 protected:
  void PostTaskImpl(std::unique_ptr<v8::Task> task,
                    const v8::SourceLocation& location) override {
    v8__Platform__CustomPlatform__BASE__PostTask(
        context_, static_cast<void*>(isolate_), task.release());
  }
  void PostNonNestableTaskImpl(std::unique_ptr<v8::Task> task,
                               const v8::SourceLocation& location) override {
    v8__Platform__CustomPlatform__BASE__PostNonNestableTask(
        context_, static_cast<void*>(isolate_), task.release());
  }
  void PostDelayedTaskImpl(std::unique_ptr<v8::Task> task,
                           double delay_in_seconds,
                           const v8::SourceLocation& location) override {
    v8__Platform__CustomPlatform__BASE__PostDelayedTask(
        context_, static_cast<void*>(isolate_), task.release(),
        delay_in_seconds > 0 ? delay_in_seconds : 0.0);
  }
  void PostNonNestableDelayedTaskImpl(
      std::unique_ptr<v8::Task> task, double delay_in_seconds,
      const v8::SourceLocation& location) override {
    v8__Platform__CustomPlatform__BASE__PostNonNestableDelayedTask(
        context_, static_cast<void*>(isolate_), task.release(),
        delay_in_seconds > 0 ? delay_in_seconds : 0.0);
  }
  void PostIdleTaskImpl(std::unique_ptr<v8::IdleTask> task,
                        const v8::SourceLocation& location) override {
    v8__Platform__CustomPlatform__BASE__PostIdleTask(
        context_, static_cast<void*>(isolate_), task.release());
  }

 private:
  std::shared_ptr<v8::TaskRunner> wrapped_;
  void* context_;
  v8::Isolate* isolate_;
};

// Platform subclass that wraps each isolate's TaskRunner to notify Rust
// when foreground tasks are posted. Follows the inspector API pattern.
//
// NotifyIsolateShutdown is NOT intercepted here because it is not virtual
// on DefaultPlatform — V8's free function does static_cast<DefaultPlatform*>
// and calls it directly, bypassing any override. Isolate cleanup must be
// handled on the Rust side (e.g. in the isolate's Drop impl).
class CustomPlatform : public v8::platform::DefaultPlatform {
  using IdleTaskSupport = v8::platform::IdleTaskSupport;

 public:
  CustomPlatform(int thread_pool_size, IdleTaskSupport idle_task_support,
                 bool unprotected, void* context)
      : DefaultPlatform(thread_pool_size, idle_task_support),
        unprotected_(unprotected),
        context_(context) {}

  // SAFETY: The platform is single-owner (via unique_ptr). The destructor
  // runs after all isolates have been disposed and no more task runner
  // callbacks can fire, so DROP does not race with other callbacks.
  ~CustomPlatform() override {
    v8__Platform__CustomPlatform__BASE__DROP(context_);
  }

  std::shared_ptr<v8::TaskRunner> GetForegroundTaskRunner(
      v8::Isolate* isolate, v8::TaskPriority priority) override {
    auto original = DefaultPlatform::GetForegroundTaskRunner(isolate, priority);
    std::lock_guard<std::mutex> lock(mutex_);
    auto key = std::make_pair(isolate, priority);
    auto it = runners_.find(key);
    if (it != runners_.end()) {
      auto runner = it->second.lock();
      if (runner) return runner;
    }
    auto custom =
        std::make_shared<CustomTaskRunner>(original, context_, isolate);
    runners_[key] = custom;
    return custom;
  }

  // When unprotected, disable thread-isolated allocations (same as
  // UnprotectedDefaultPlatform). Required when isolates may be created on
  // threads other than the one that called v8::V8::Initialize (e.g. worker
  // threads in Deno).
  v8::ThreadIsolatedAllocator* GetThreadIsolatedAllocator() override {
    if (unprotected_) return nullptr;
    return DefaultPlatform::GetThreadIsolatedAllocator();
  }

 private:
  bool unprotected_;
  void* context_;
  std::mutex mutex_;
  // weak_ptr so runners are kept alive only while V8 holds a reference.
  // When V8 drops its shared_ptr (e.g. on isolate shutdown), the weak_ptr
  // expires and a fresh wrapper is created if GetForegroundTaskRunner is
  // called again. This avoids preventing cleanup of the underlying runner.
  std::map<std::pair<v8::Isolate*, v8::TaskPriority>,
           std::weak_ptr<CustomTaskRunner>>
      runners_;
};

class UnprotectedDefaultPlatform : public v8::platform::DefaultPlatform {
  using IdleTaskSupport = v8::platform::IdleTaskSupport;
  using InProcessStackDumping = v8::platform::InProcessStackDumping;
  using PriorityMode = v8::platform::PriorityMode;
  using TracingController = v8::TracingController;

 public:
  explicit UnprotectedDefaultPlatform(
      int thread_pool_size, IdleTaskSupport idle_task_support,
      std::unique_ptr<TracingController> tracing_controller = {},
      PriorityMode priority_mode = PriorityMode::kDontApply)
      : v8::platform::DefaultPlatform(thread_pool_size, idle_task_support,
                                      std::move(tracing_controller),
                                      priority_mode) {}

  v8::ThreadIsolatedAllocator* GetThreadIsolatedAllocator() override {
    // DefaultThreadIsolatedAllocator is PKU protected
    return nullptr;
  }
};

v8::Platform* v8__Platform__NewDefaultPlatform(int thread_pool_size,
                                               bool idle_task_support) {
  return v8::platform::NewDefaultPlatform(
             thread_pool_size,
             idle_task_support ? v8::platform::IdleTaskSupport::kEnabled
                               : v8::platform::IdleTaskSupport::kDisabled,
             v8::platform::InProcessStackDumping::kDisabled, nullptr)
      .release();
}

v8::Platform* v8__Platform__NewUnprotectedDefaultPlatform(
    int thread_pool_size, bool idle_task_support) {
  if (thread_pool_size < 1) {
    thread_pool_size = std::thread::hardware_concurrency();
  }
  thread_pool_size = std::max(std::min(thread_pool_size, 16), 1);
  return std::make_unique<UnprotectedDefaultPlatform>(
             thread_pool_size, idle_task_support
                                   ? v8::platform::IdleTaskSupport::kEnabled
                                   : v8::platform::IdleTaskSupport::kDisabled)
      .release();
}

v8::Platform* v8__Platform__NewSingleThreadedDefaultPlatform(
    bool idle_task_support) {
  return v8::platform::NewSingleThreadedDefaultPlatform(
             idle_task_support ? v8::platform::IdleTaskSupport::kEnabled
                               : v8::platform::IdleTaskSupport::kDisabled,
             v8::platform::InProcessStackDumping::kDisabled, nullptr)
      .release();
}

v8::Platform* v8__Platform__NewCustomPlatform(int thread_pool_size,
                                              bool idle_task_support,
                                              bool unprotected, void* context) {
  if (thread_pool_size < 1) {
    thread_pool_size = std::thread::hardware_concurrency();
  }
  thread_pool_size = std::max(std::min(thread_pool_size, 16), 1);
  return std::make_unique<CustomPlatform>(
             thread_pool_size,
             idle_task_support ? v8::platform::IdleTaskSupport::kEnabled
                               : v8::platform::IdleTaskSupport::kDisabled,
             unprotected, context)
      .release();
}

bool v8__Platform__PumpMessageLoop(v8::Platform* platform, v8::Isolate* isolate,
                                   bool wait_for_work) {
  return v8::platform::PumpMessageLoop(
      platform, isolate,
      wait_for_work ? v8::platform::MessageLoopBehavior::kWaitForWork
                    : v8::platform::MessageLoopBehavior::kDoNotWait);
}

void v8__Platform__RunIdleTasks(v8::Platform* platform, v8::Isolate* isolate,
                                double idle_time_in_seconds) {
  v8::platform::RunIdleTasks(platform, isolate, idle_time_in_seconds);
}

void v8__Platform__NotifyIsolateShutdown(v8::Platform* platform,
                                         v8::Isolate* isolate) {
  v8::platform::NotifyIsolateShutdown(platform, isolate);
}

void v8__Platform__DELETE(v8::Platform* self) { delete self; }

two_pointers_t std__shared_ptr__v8__Platform__CONVERT__std__unique_ptr(
    v8::Platform* unique_ptr) {
  return make_pod<two_pointers_t>(std::shared_ptr<v8::Platform>(unique_ptr));
}

v8::Platform* std__shared_ptr__v8__Platform__get(
    const std::shared_ptr<v8::Platform>& ptr) {
  return ptr.get();
}

two_pointers_t std__shared_ptr__v8__Platform__COPY(
    const std::shared_ptr<v8::Platform>& ptr) {
  return make_pod<two_pointers_t>(ptr);
}

void std__shared_ptr__v8__Platform__reset(std::shared_ptr<v8::Platform>* ptr) {
  ptr->reset();
}

long std__shared_ptr__v8__Platform__use_count(
    const std::shared_ptr<v8::Platform>& ptr) {
  return ptr.use_count();
}

void v8_inspector__V8Inspector__Channel__BASE__sendResponse(
    v8_inspector::V8Inspector::Channel* self, int callId,
    v8_inspector::StringBuffer* message);
void v8_inspector__V8Inspector__Channel__BASE__sendNotification(
    v8_inspector::V8Inspector::Channel* self,
    v8_inspector::StringBuffer* message);
void v8_inspector__V8Inspector__Channel__BASE__flushProtocolNotifications(
    v8_inspector::V8Inspector::Channel* self);

void v8_inspector__V8Inspector__DELETE(v8_inspector::V8Inspector* self) {
  delete self;
}

v8_inspector::V8Inspector* v8_inspector__V8Inspector__create(
    v8::Isolate* isolate, v8_inspector::V8InspectorClient* client) {
  std::unique_ptr<v8_inspector::V8Inspector> u =
      v8_inspector::V8Inspector::create(isolate, client);
  return u.release();
}

v8_inspector::V8InspectorSession* v8_inspector__V8Inspector__connect(
    v8_inspector::V8Inspector* self, int context_group_id,
    v8_inspector::V8Inspector::Channel* channel, v8_inspector::StringView state,
    v8_inspector::V8Inspector::ClientTrustLevel client_trust_level) {
  std::unique_ptr<v8_inspector::V8InspectorSession> u =
      self->connect(context_group_id, channel, state, client_trust_level);
  return u.release();
}

void v8_inspector__V8Inspector__contextCreated(
    v8_inspector::V8Inspector* self, const v8::Context& context,
    int contextGroupId, v8_inspector::StringView humanReadableName,
    v8_inspector::StringView auxData) {
  v8_inspector::V8ContextInfo info(ptr_to_local(&context), contextGroupId,
                                   humanReadableName);
  info.auxData = auxData;
  self->contextCreated(info);
}

void v8_inspector__V8Inspector__contextDestroyed(
    v8_inspector::V8Inspector* self, const v8::Context& context) {
  self->contextDestroyed(ptr_to_local(&context));
}

void v8_inspector__V8Inspector__idleStarted(v8_inspector::V8Inspector* self) {
  self->idleStarted();
}

void v8_inspector__V8Inspector__idleFinished(v8_inspector::V8Inspector* self) {
  self->idleFinished();
}

void v8_inspector__V8Inspector__asyncTaskScheduled(
    v8_inspector::V8Inspector* self, v8_inspector::StringView task_name,
    void* task, bool recurring) {
  self->asyncTaskScheduled(task_name, task, recurring);
}

void v8_inspector__V8Inspector__asyncTaskCanceled(
    v8_inspector::V8Inspector* self, void* task) {
  self->asyncTaskCanceled(task);
}

void v8_inspector__V8Inspector__asyncTaskStarted(
    v8_inspector::V8Inspector* self, void* task) {
  self->asyncTaskStarted(task);
}

void v8_inspector__V8Inspector__asyncTaskFinished(
    v8_inspector::V8Inspector* self, void* task) {
  self->asyncTaskFinished(task);
}

void v8_inspector__V8Inspector__allAsyncTasksCanceled(
    v8_inspector::V8Inspector* self) {
  self->allAsyncTasksCanceled();
}

bool v8_inspector__V8InspectorSession__canDispatchMethod(
    v8_inspector::StringView method) {
  return v8_inspector::V8InspectorSession::canDispatchMethod(method);
}

unsigned v8_inspector__V8Inspector__exceptionThrown(
    v8_inspector::V8Inspector* self, const v8::Context& context,
    v8_inspector::StringView message, const v8::Value& exception,
    v8_inspector::StringView detailed_message, v8_inspector::StringView url,
    unsigned line_number, unsigned column_number,
    v8_inspector::V8StackTrace* stack_trace, int script_id) {
  return self->exceptionThrown(
      ptr_to_local(&context), message, ptr_to_local(&exception),
      detailed_message, url, line_number, column_number,
      static_cast<std::unique_ptr<v8_inspector::V8StackTrace>>(stack_trace),
      script_id);
}

v8_inspector::V8StackTrace* v8_inspector__V8Inspector__createStackTrace(
    v8_inspector::V8Inspector* self, const v8::StackTrace& stack_trace) {
  std::unique_ptr<v8_inspector::V8StackTrace> u =
      self->createStackTrace(ptr_to_local(&stack_trace));
  return u.release();
}

void v8_inspector__V8StackTrace__DELETE(v8_inspector::V8StackTrace* self) {
  delete self;
}

void v8_inspector__V8InspectorSession__DELETE(
    v8_inspector::V8InspectorSession* self) {
  delete self;
}

void v8_inspector__V8InspectorSession__dispatchProtocolMessage(
    v8_inspector::V8InspectorSession* self, v8_inspector::StringView message) {
  self->dispatchProtocolMessage(message);
}

void v8_inspector__V8InspectorSession__releaseObjectGroup(
    v8_inspector::V8InspectorSession* self,
    v8_inspector::StringView object_group) {
  self->releaseObjectGroup(object_group);
}

v8_inspector::protocol::Runtime::API::RemoteObject*
v8_inspector__V8InspectorSession__wrapObject(
    v8_inspector::V8InspectorSession* self, const v8::Context* context,
    const v8::Value* value, v8_inspector::StringView object_group,
    bool generate_preview) {
  return self
      ->wrapObject(ptr_to_local(context), ptr_to_local(value), object_group,
                   generate_preview)
      .release();
}

bool v8_inspector__V8InspectorSession__unwrapObject(
    v8_inspector::V8InspectorSession* self, v8_inspector::StringBuffer** error,
    v8_inspector::StringView object_id, const v8::Value** value,
    const v8::Context** context, v8_inspector::StringBuffer** object_group) {
  std::unique_ptr<v8_inspector::StringBuffer> error_buffer;
  v8::Local<v8::Value> local_value;
  v8::Local<v8::Context> local_context;
  std::unique_ptr<v8_inspector::StringBuffer> object_group_buffer;
  bool success = self->unwrapObject(&error_buffer, object_id, &local_value,
                                    &local_context, &object_group_buffer);
  if (!success) {
    *error = error_buffer.release();
    return false;
  }

  *value = local_to_ptr(local_value);
  *context = local_to_ptr(local_context);
  *object_group = object_group_buffer.release();
  return true;
}

void v8_inspector__RemoteObject__DELETE(
    v8_inspector::protocol::Runtime::API::RemoteObject* self) {
  delete self;
}

void v8_inspector__V8InspectorSession__schedulePauseOnNextStatement(
    v8_inspector::V8InspectorSession* self, v8_inspector::StringView reason,
    v8_inspector::StringView detail) {
  self->schedulePauseOnNextStatement(reason, detail);
}

void v8_inspector__V8InspectorSession__cancelPauseOnNextStatement(
    v8_inspector::V8InspectorSession* self) {
  self->cancelPauseOnNextStatement();
}
}  // extern "C"

extern "C" {
const v8::Value* v8_inspector__V8InspectorSession__Inspectable__BASE__get(
    void* rust_impl, const v8::Context* context);
void v8_inspector__V8InspectorSession__Inspectable__BASE__DROP(void* rust_impl);
}

class v8_inspector__V8InspectorSession__Inspectable__BASE final
    : public v8_inspector::V8InspectorSession::Inspectable {
 public:
  explicit v8_inspector__V8InspectorSession__Inspectable__BASE(void* rust_impl)
      : rust_impl_(rust_impl) {}

  ~v8_inspector__V8InspectorSession__Inspectable__BASE() override {
    v8_inspector__V8InspectorSession__Inspectable__BASE__DROP(rust_impl_);
  }

  v8::Local<v8::Value> get(v8::Local<v8::Context> context) override {
    // The Rust CallbackScope relies on `NewCallbackScope for Local<Context>`
    // having `NEEDS_SCOPE == false` and must not open its own HandleScope.
    // Handles created by the Rust implementation are allocated in this scope
    // and must be escaped here. Opening a nested HandleScope in Rust would
    // destroy that scope before Escape uses its handle, causing a
    // use-after-free.
    v8::Isolate* isolate = v8::Isolate::GetCurrent();
    if (isolate == nullptr) return v8::Local<v8::Value>();
    v8::EscapableHandleScope handle_scope(isolate);
    v8::Context::Scope context_scope(context);
    return handle_scope.Escape(
        ptr_to_local(v8_inspector__V8InspectorSession__Inspectable__BASE__get(
            rust_impl_, local_to_ptr(context))));
  }

 private:
  void* rust_impl_;
};

extern "C" {
v8_inspector::V8InspectorSession::Inspectable*
v8_inspector__V8InspectorSession__Inspectable__NEW(void* rust_impl) {
  return new v8_inspector__V8InspectorSession__Inspectable__BASE(rust_impl);
}

void v8_inspector__V8InspectorSession__Inspectable__DELETE(
    v8_inspector::V8InspectorSession::Inspectable* inspectable) {
  delete inspectable;
}

void v8_inspector__V8InspectorSession__addInspectedObject(
    v8_inspector::V8InspectorSession* self,
    v8_inspector::V8InspectorSession::Inspectable* inspectable) {
  self->addInspectedObject(
      std::unique_ptr<v8_inspector::V8InspectorSession::Inspectable>(
          inspectable));
}
}

struct v8_inspector__V8Inspector__Channel__BASE
    : public v8_inspector::V8Inspector::Channel {
  using v8_inspector::V8Inspector::Channel::Channel;

  void sendResponse(
      int callId,
      std::unique_ptr<v8_inspector::StringBuffer> message) override {
    v8_inspector__V8Inspector__Channel__BASE__sendResponse(this, callId,
                                                           message.release());
  }
  void sendNotification(
      std::unique_ptr<v8_inspector::StringBuffer> message) override {
    v8_inspector__V8Inspector__Channel__BASE__sendNotification(
        this, message.release());
  }
  void flushProtocolNotifications() override {
    v8_inspector__V8Inspector__Channel__BASE__flushProtocolNotifications(this);
  }
};

extern "C" {
void v8_inspector__V8Inspector__Channel__BASE__CONSTRUCT(
    uninit_t<v8_inspector__V8Inspector__Channel__BASE>* buf) {
  construct_in_place<v8_inspector__V8Inspector__Channel__BASE>(buf);
}

void v8_inspector__V8Inspector__Channel__sendResponse(
    v8_inspector::V8Inspector::Channel* self, int callId,
    v8_inspector::StringBuffer* message) {
  self->sendResponse(
      callId,
      static_cast<std::unique_ptr<v8_inspector::StringBuffer>>(message));
}
void v8_inspector__V8Inspector__Channel__sendNotification(
    v8_inspector::V8Inspector::Channel* self,
    v8_inspector::StringBuffer* message) {
  self->sendNotification(
      static_cast<std::unique_ptr<v8_inspector::StringBuffer>>(message));
}
void v8_inspector__V8Inspector__Channel__flushProtocolNotifications(
    v8_inspector::V8Inspector::Channel* self) {
  self->flushProtocolNotifications();
}

int64_t v8_inspector__V8InspectorClient__BASE__generateUniqueId(
    v8_inspector::V8InspectorClient* self);
void v8_inspector__V8InspectorClient__BASE__runMessageLoopOnPause(
    v8_inspector::V8InspectorClient* self, int contextGroupId);
void v8_inspector__V8InspectorClient__BASE__quitMessageLoopOnPause(
    v8_inspector::V8InspectorClient* self);
void v8_inspector__V8InspectorClient__BASE__runIfWaitingForDebugger(
    v8_inspector::V8InspectorClient* self, int contextGroupId);
void v8_inspector__V8InspectorClient__BASE__consoleAPIMessage(
    v8_inspector::V8InspectorClient* self, int contextGroupId,
    v8::Isolate::MessageErrorLevel level,
    const v8_inspector::StringView& message,
    const v8_inspector::StringView& url, unsigned lineNumber,
    unsigned columnNumber, v8_inspector::V8StackTrace* stackTrace);
v8_inspector::StringBuffer* v8_inspector__V8InspectorClient__BASE__valueSubtype(
    v8_inspector::V8InspectorClient* self, v8::Context* context,
    v8::Value* value);
v8_inspector::StringBuffer*
v8_inspector__V8InspectorClient__BASE__descriptionForValueSubtype(
    v8_inspector::V8InspectorClient* self, v8::Context* context,
    v8::Value* value);
v8::Context* v8_inspector__V8InspectorClient__BASE__ensureDefaultContextInGroup(
    v8_inspector::V8InspectorClient* self, int context_group_id);
v8_inspector::StringBuffer*
v8_inspector__V8InspectorClient__BASE__resourceNameToUrl(
    v8_inspector::V8InspectorClient* self,
    const v8_inspector::StringView& resource_name_view);

}  // extern "C"

struct v8_inspector__V8InspectorClient__BASE
    : public v8_inspector::V8InspectorClient {
  using v8_inspector::V8InspectorClient::V8InspectorClient;

  int64_t generateUniqueId() override {
    return v8_inspector__V8InspectorClient__BASE__generateUniqueId(this);
  }
  void runMessageLoopOnPause(int contextGroupId) override {
    v8_inspector__V8InspectorClient__BASE__runMessageLoopOnPause(
        this, contextGroupId);
  }
  void quitMessageLoopOnPause() override {
    v8_inspector__V8InspectorClient__BASE__quitMessageLoopOnPause(this);
  }
  void runIfWaitingForDebugger(int contextGroupId) override {
    v8_inspector__V8InspectorClient__BASE__runIfWaitingForDebugger(
        this, contextGroupId);
  }
  void consoleAPIMessage(int contextGroupId,
                         v8::Isolate::MessageErrorLevel level,
                         const v8_inspector::StringView& message,
                         const v8_inspector::StringView& url,
                         unsigned lineNumber, unsigned columnNumber,
                         v8_inspector::V8StackTrace* stackTrace) override {
    v8_inspector__V8InspectorClient__BASE__consoleAPIMessage(
        this, contextGroupId, level, message, url, lineNumber, columnNumber,
        stackTrace);
  }
  std::unique_ptr<v8_inspector::StringBuffer> valueSubtype(
      v8::Local<v8::Value> value) override {
    v8::Isolate* isolate = v8::Isolate::GetCurrent();
    if (isolate == nullptr) return nullptr;
    v8::Local<v8::Context> context = isolate->GetCurrentContext();
    if (context.IsEmpty()) return nullptr;
    v8_inspector::StringBuffer* b =
        v8_inspector__V8InspectorClient__BASE__valueSubtype(
            this, local_to_ptr(context), local_to_ptr(value));
    return std::unique_ptr<v8_inspector::StringBuffer>(b);
  }
  std::unique_ptr<v8_inspector::StringBuffer> descriptionForValueSubtype(
      v8::Local<v8::Context> context, v8::Local<v8::Value> value) override {
    v8_inspector::StringBuffer* b =
        v8_inspector__V8InspectorClient__BASE__descriptionForValueSubtype(
            this, local_to_ptr(context), local_to_ptr(value));
    return std::unique_ptr<v8_inspector::StringBuffer>(b);
  }
  v8::Local<v8::Context> ensureDefaultContextInGroup(
      int context_group_id) override {
    return ptr_to_local(
        v8_inspector__V8InspectorClient__BASE__ensureDefaultContextInGroup(
            this, context_group_id));
  }
  std::unique_ptr<v8_inspector::StringBuffer> resourceNameToUrl(
      const v8_inspector::StringView& resource_name_view) override {
    v8_inspector::StringBuffer* b =
        v8_inspector__V8InspectorClient__BASE__resourceNameToUrl(
            this, resource_name_view);
    return std::unique_ptr<v8_inspector::StringBuffer>(b);
  }
};

extern "C" {
void v8_inspector__V8InspectorClient__BASE__CONSTRUCT(
    uninit_t<v8_inspector__V8InspectorClient__BASE>* buf) {
  construct_in_place<v8_inspector__V8InspectorClient__BASE>(buf);
}

int64_t v8_inspector__V8InspectorClient__generateUniqueId(
    v8_inspector::V8InspectorClient* self) {
  return self->generateUniqueId();
}

void v8_inspector__V8InspectorClient__runMessageLoopOnPause(
    v8_inspector::V8InspectorClient* self, int contextGroupId) {
  self->runMessageLoopOnPause(contextGroupId);
}
void v8_inspector__V8InspectorClient__quitMessageLoopOnPause(
    v8_inspector::V8InspectorClient* self) {
  self->quitMessageLoopOnPause();
}
void v8_inspector__V8InspectorClient__runIfWaitingForDebugger(
    v8_inspector::V8InspectorClient* self, int contextGroupId) {
  self->runIfWaitingForDebugger(contextGroupId);
}

void v8_inspector__V8InspectorClient__consoleAPIMessage(
    v8_inspector::V8InspectorClient* self, int contextGroupId,
    v8::Isolate::MessageErrorLevel level,
    const v8_inspector::StringView& message,
    const v8_inspector::StringView& url, unsigned lineNumber,
    unsigned columnNumber, v8_inspector::V8StackTrace* stackTrace) {
  self->consoleAPIMessage(contextGroupId, level, message, url, lineNumber,
                          columnNumber, stackTrace);
}

void v8_inspector__StringBuffer__DELETE(v8_inspector::StringBuffer* self) {
  delete self;
}

three_pointers_t v8_inspector__StringBuffer__string(
    const v8_inspector::StringBuffer& self) {
  return make_pod<three_pointers_t>(self.string());
}

v8_inspector::StringBuffer* v8_inspector__StringBuffer__create(
    v8_inspector::StringView source) {
  return v8_inspector::StringBuffer::create(source).release();
}

int v8__Location__GetLineNumber(v8::Location* self) {
  return self->GetLineNumber();
}

int v8__Location__GetColumnNumber(v8::Location* self) {
  return self->GetColumnNumber();
}

v8::Module::Status v8__Module__GetStatus(const v8::Module& self) {
  return self.GetStatus();
}

const v8::Value* v8__Module__GetException(const v8::Module& self) {
  return local_to_ptr(self.GetException());
}

const v8::FixedArray* v8__Module__GetModuleRequests(const v8::Module& self) {
  return local_to_ptr(self.GetModuleRequests());
}

void v8__Module__SourceOffsetToLocation(const v8::Module& self, int offset,
                                        v8::Location* out) {
  *out = self.SourceOffsetToLocation(offset);
}

const v8::Value* v8__Module__GetModuleNamespace(const v8::Module& self) {
  return local_to_ptr(ptr_to_local(&self)->GetModuleNamespace());
}

const v8::Value* v8__Module__GetModuleNamespace2(const v8::Module& self,
                                                 v8::ModuleImportPhase phase) {
  return local_to_ptr(ptr_to_local(&self)->GetModuleNamespace(phase));
}

const v8::Value* v8__Module__EvaluateForImportDefer(
    const v8::Module& self, const v8::Context& context) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->EvaluateForImportDefer(ptr_to_local(&context)));
}

int v8__Module__GetIdentityHash(const v8::Module& self) {
  return self.GetIdentityHash();
}

int v8__Module__ScriptId(const v8::Module& self) {
  // Module::ScriptId() isn't marked const but its implementation is
  // so this const_cast is sound.
  // TODO(bnoordhuis) Open V8 CL to mark Module::ScriptId() and
  // UnboundScript::GetId() const.
  return const_cast<v8::Module&>(self).ScriptId();
}

MaybeBool v8__Module__InstantiateModule(
    const v8::Module& self, const v8::Context& context,
    v8::Module::ResolveModuleCallback cb,
    v8::Module::ResolveSourceCallback source_callback) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->InstantiateModule(
      ptr_to_local(&context), cb, source_callback));
}

const v8::Value* v8__Module__Evaluate(const v8::Module& self,
                                      const v8::Context& context) {
  return maybe_local_to_ptr(
      ptr_to_local(&self)->Evaluate(ptr_to_local(&context)));
}

bool v8__Module__IsGraphAsync(const v8::Module& self) {
  return ptr_to_local(&self)->IsGraphAsync();
}

bool v8__Module__HasTopLevelAwait(const v8::Module& self) {
  return ptr_to_local(&self)->HasTopLevelAwait();
}

bool v8__Module__IsSourceTextModule(const v8::Module& self) {
  return ptr_to_local(&self)->IsSourceTextModule();
}

bool v8__Module__IsSyntheticModule(const v8::Module& self) {
  return ptr_to_local(&self)->IsSyntheticModule();
}

const v8::Module* v8__Module__CreateSyntheticModule(
    v8::Isolate* isolate, const v8::String* module_name,
    size_t export_names_len, const v8::String* export_names_raw[],
    v8::Module::SyntheticModuleEvaluationSteps evaluation_steps) {
  std::vector<v8::Local<v8::String>> export_names_vec{};
  for (size_t i = 0; i < export_names_len; i += 1) {
    export_names_vec.push_back(ptr_to_local(export_names_raw[i]));
  }
  auto export_names = v8::MemorySpan<const v8::Local<v8::String>>{
      export_names_vec.data(), export_names_len};
  return local_to_ptr(v8::Module::CreateSyntheticModule(
      isolate, ptr_to_local(module_name), export_names, evaluation_steps));
}

MaybeBool v8__Module__SetSyntheticModuleExport(const v8::Module& self,
                                               v8::Isolate* isolate,
                                               const v8::String* export_name,
                                               const v8::Value* export_value) {
  return maybe_to_maybe_bool(ptr_to_local(&self)->SetSyntheticModuleExport(
      isolate, ptr_to_local(export_name), ptr_to_local(export_value)));
}

const v8::UnboundModuleScript* v8__Module__GetUnboundModuleScript(
    const v8::Module& self) {
  return local_to_ptr(ptr_to_local(&self)->GetUnboundModuleScript());
}

struct StalledTopLevelAwaitMessage {
  const v8::Module* module;
  const v8::Message* message;
};

size_t v8__Module__GetStalledTopLevelAwaitMessage(
    const v8::Module& self, v8::Isolate* isolate,
    StalledTopLevelAwaitMessage* out_vec, size_t out_len) {
  auto [modules, messages] =
      ptr_to_local(&self)->GetStalledTopLevelAwaitMessages(isolate);
  auto len = std::min(messages.size(), out_len);
  for (size_t i = 0; i < len; i += 1) {
    StalledTopLevelAwaitMessage stalled_message;
    stalled_message.module = local_to_ptr(modules[i]);
    stalled_message.message = local_to_ptr(messages[i]);
    out_vec[i] = stalled_message;
  }
  return len;
}

const v8::String* v8__ModuleRequest__GetSpecifier(
    const v8::ModuleRequest& self) {
  return local_to_ptr(self.GetSpecifier());
}

v8::ModuleImportPhase v8__ModuleRequest__GetPhase(
    const v8::ModuleRequest& self) {
  return self.GetPhase();
}

int v8__ModuleRequest__GetSourceOffset(const v8::ModuleRequest& self) {
  return self.GetSourceOffset();
}

const v8::FixedArray* v8__ModuleRequest__GetImportAttributes(
    const v8::ModuleRequest& self) {
  return local_to_ptr(self.GetImportAttributes());
}

struct WasmStreamingSharedPtr {
  std::shared_ptr<v8::WasmStreaming> inner;
};

static_assert(sizeof(WasmStreamingSharedPtr) <= 2 * sizeof(void*),
              "std::shared_ptr<v8::WasmStreaming> size mismatch");

void v8__WasmStreaming__Unpack(v8::Isolate* isolate, const v8::Value& value,
                               WasmStreamingSharedPtr* self) {
  new (self) WasmStreamingSharedPtr();
  self->inner = v8::WasmStreaming::Unpack(isolate, ptr_to_local(&value));
}

void v8__WasmStreaming__shared_ptr_DESTRUCT(WasmStreamingSharedPtr* self) {
  self->~WasmStreamingSharedPtr();
}

void v8__WasmStreaming__SetHasCompiledModuleBytes(
    WasmStreamingSharedPtr* self) {
  self->inner->SetHasCompiledModuleBytes();
}

void v8__WasmStreaming__OnBytesReceived(WasmStreamingSharedPtr* self,
                                        const uint8_t* data, size_t len) {
  self->inner->OnBytesReceived(data, len);
}

void v8__WasmStreaming__Finish(
    WasmStreamingSharedPtr* self,
    void (*callback)(v8::WasmStreaming::ModuleCachingInterface&)) {
  self->inner->Finish(callback);
}

void v8__WasmStreaming__Abort(WasmStreamingSharedPtr* self,
                              const v8::Value* exception) {
  self->inner->Abort(ptr_to_maybe_local(exception));
}

void v8__WasmStreaming__SetUrl(WasmStreamingSharedPtr* self, const char* url,
                               size_t len) {
  self->inner->SetUrl(url, len);
}

const_memory_span_t v8__ModuleCachingInterface__GetWireBytes(
    const v8::WasmStreaming::ModuleCachingInterface& self) {
  v8::MemorySpan<const uint8_t> span = self.GetWireBytes();
  return {span.data(), span.size()};
}

bool v8__ModuleCachingInterface__SetCachedCompiledModuleBytes(
    v8::WasmStreaming::ModuleCachingInterface& self,
    const_memory_span_t bytes) {
  v8::MemorySpan<const uint8_t> bytes_span{bytes.data, bytes.size};
  return self.SetCachedCompiledModuleBytes(bytes_span);
}

const v8::ArrayBuffer* v8__WasmMemoryObject__Buffer(
    const v8::WasmMemoryObject& self) {
  return local_to_ptr(ptr_to_local(&self)->Buffer());
}

using HeapSnapshotCallback = bool (*)(void*, const char*, size_t);

void v8__HeapProfiler__TakeHeapSnapshot(v8::Isolate* isolate,
                                        HeapSnapshotCallback callback,
                                        void* arg) {
  struct OutputStream : public v8::OutputStream {
    OutputStream(HeapSnapshotCallback callback, void* arg)
        : callback_(callback), arg_(arg) {}
    void EndOfStream() override {
      static_cast<void>(callback_(arg_, nullptr, 0));
    }
    v8::OutputStream::WriteResult WriteAsciiChunk(char* data,
                                                  int size) override {
      assert(size >= 0);  // Can never be < 0 barring bugs in V8.
      if (callback_(arg_, data, static_cast<size_t>(size)))
        return v8::OutputStream::kContinue;
      return v8::OutputStream::kAbort;
    }
    HeapSnapshotCallback const callback_;
    void* const arg_;
  };

  const v8::HeapSnapshot* snapshot =
      isolate->GetHeapProfiler()->TakeHeapSnapshot();
  if (snapshot == nullptr) return;  // Snapshotting failed, probably OOM.
  OutputStream stream(callback, arg);
  snapshot->Serialize(&stream);
  // We don't want to call HeapProfiler::DeleteAllHeapSnapshots() because that
  // invalidates snapshots we don't own. The const_cast hack has been in use
  // in node-heapdump for the last 8 years and I think there is a pretty
  // good chance it'll keep working for 8 more.
  const_cast<v8::HeapSnapshot*>(snapshot)->Delete();
}

void v8__CpuProfiler__CollectSample(v8::Isolate* isolate,
                                    const uint64_t* trace_id) {
  if (trace_id == nullptr) {
    v8::CpuProfiler::CollectSample(isolate);
  } else {
    v8::CpuProfiler::CollectSample(isolate, *trace_id);
  }
}

void v8__CpuProfiler__UseDetailedSourcePositionsForProfiling(
    v8::Isolate* isolate) {
  v8::CpuProfiler::UseDetailedSourcePositionsForProfiling(isolate);
}

}  // extern "C"

// v8::ValueSerializer::Delegate

extern "C" {
void v8__ValueSerializer__Delegate__ThrowDataCloneError(
    v8::ValueSerializer::Delegate* self, v8::Local<v8::String> message);

bool v8__ValueSerializer__Delegate__HasCustomHostObject(
    v8::ValueSerializer::Delegate* self, v8::Isolate* isolate);

MaybeBool v8__ValueSerializer__Delegate__IsHostObject(
    v8::ValueSerializer::Delegate* self, v8::Isolate* isolate,
    v8::Local<v8::Object> object);

MaybeBool v8__ValueSerializer__Delegate__WriteHostObject(
    v8::ValueSerializer::Delegate* self, v8::Isolate* isolate,
    v8::Local<v8::Object> object);

bool v8__ValueSerializer__Delegate__GetSharedArrayBufferId(
    v8::ValueSerializer::Delegate* self, v8::Isolate* isolate,
    v8::Local<v8::SharedArrayBuffer> shared_array_buffer, uint32_t* result);

bool v8__ValueSerializer__Delegate__GetWasmModuleTransferId(
    v8::ValueSerializer::Delegate* self, v8::Isolate* isolate,
    v8::Local<v8::WasmModuleObject> module, uint32_t* result);

void* v8__ValueSerializer__Delegate__ReallocateBufferMemory(
    v8::ValueSerializer::Delegate* self, void* old_buffer, size_t size,
    size_t* actual_size);

void v8__ValueSerializer__Delegate__FreeBufferMemory(
    v8::ValueSerializer::Delegate* self, void* buffer);
}

struct v8__ValueSerializer__Delegate : public v8::ValueSerializer::Delegate {
  void ThrowDataCloneError(v8::Local<v8::String> message) override {
    v8__ValueSerializer__Delegate__ThrowDataCloneError(this, message);
  }

  bool HasCustomHostObject(v8::Isolate* isolate) override {
    return v8__ValueSerializer__Delegate__HasCustomHostObject(this, isolate);
  }

  v8::Maybe<bool> IsHostObject(v8::Isolate* isolate,
                               v8::Local<v8::Object> object) override {
    return maybe_bool_to_maybe(
        v8__ValueSerializer__Delegate__IsHostObject(this, isolate, object));
  }

  v8::Maybe<bool> WriteHostObject(v8::Isolate* isolate,
                                  v8::Local<v8::Object> object) override {
    return maybe_bool_to_maybe(
        v8__ValueSerializer__Delegate__WriteHostObject(this, isolate, object));
  }

  v8::Maybe<uint32_t> GetSharedArrayBufferId(
      v8::Isolate* isolate,
      v8::Local<v8::SharedArrayBuffer> shared_array_buffer) override {
    uint32_t result = 0;
    if (!v8__ValueSerializer__Delegate__GetSharedArrayBufferId(
            this, isolate, shared_array_buffer, &result)) {
      // Forward to the original method. It'll throw DataCloneError.
      return v8::ValueSerializer::Delegate::GetSharedArrayBufferId(
          isolate, shared_array_buffer);
    }
    return v8::Just(result);
  }

  v8::Maybe<uint32_t> GetWasmModuleTransferId(
      v8::Isolate* isolate, v8::Local<v8::WasmModuleObject> module) override {
    uint32_t result = 0;
    if (!v8__ValueSerializer__Delegate__GetWasmModuleTransferId(
            this, isolate, module, &result))
      return v8::Nothing<uint32_t>();
    return v8::Just(result);
  }

  void* ReallocateBufferMemory(void* old_buffer, size_t size,
                               size_t* actual_size) override {
    return v8__ValueSerializer__Delegate__ReallocateBufferMemory(
        this, old_buffer, size, actual_size);
  }

  void FreeBufferMemory(void* buffer) override {
    v8__ValueSerializer__Delegate__FreeBufferMemory(this, buffer);
  }
};

extern "C" {
void v8__ValueSerializer__Delegate__CONSTRUCT(
    uninit_t<v8__ValueSerializer__Delegate>* buf) {
  static_assert(sizeof(v8__ValueSerializer__Delegate) == sizeof(size_t),
                "v8__ValueSerializer__Delegate size mismatch");
  construct_in_place<v8__ValueSerializer__Delegate>(buf);
}
}

// v8::ValueSerializer

extern "C" {
void v8__ValueSerializer__CONSTRUCT(uninit_t<v8::ValueSerializer>* buf,
                                    v8::Isolate* isolate,
                                    v8::ValueSerializer::Delegate* delegate) {
  static_assert(sizeof(v8::ValueSerializer) == sizeof(size_t),
                "v8::ValueSerializer size mismatch");
  construct_in_place<v8::ValueSerializer>(buf, isolate, delegate);
}

void v8__ValueSerializer__DESTRUCT(v8::ValueSerializer* self) {
  self->~ValueSerializer();
}

void v8__ValueSerializer__Release(v8::ValueSerializer* self, uint8_t** ptr,
                                  size_t* size) {
  auto result = self->Release();
  *ptr = result.first;
  *size = result.second;
}

void v8__ValueSerializer__SetTreatArrayBufferViewsAsHostObjects(
    v8::ValueSerializer* self, bool mode) {
  self->SetTreatArrayBufferViewsAsHostObjects(mode);
}

void v8__ValueSerializer__WriteHeader(v8::ValueSerializer* self) {
  self->WriteHeader();
}

MaybeBool v8__ValueSerializer__WriteValue(v8::ValueSerializer* self,
                                          v8::Local<v8::Context> context,
                                          v8::Local<v8::Value> value) {
  return maybe_to_maybe_bool(self->WriteValue(context, value));
}

void v8__ValueSerializer__TransferArrayBuffer(
    v8::ValueSerializer* self, uint32_t transfer_id,
    v8::Local<v8::ArrayBuffer> array_buffer) {
  self->TransferArrayBuffer(transfer_id, array_buffer);
}

void v8__ValueSerializer__WriteUint32(v8::ValueSerializer* self,
                                      uint32_t value) {
  self->WriteUint32(value);
}

void v8__ValueSerializer__WriteUint64(v8::ValueSerializer* self,
                                      uint64_t value) {
  self->WriteUint64(value);
}

void v8__ValueSerializer__WriteDouble(v8::ValueSerializer* self, double value) {
  self->WriteDouble(value);
}

void v8__ValueSerializer__WriteRawBytes(v8::ValueSerializer* self,
                                        const void* source, size_t length) {
  self->WriteRawBytes(source, length);
}
}

// v8::ValueDeserializer::Delegate

extern "C" {
v8::Object* v8__ValueDeserializer__Delegate__ReadHostObject(
    v8::ValueDeserializer::Delegate* self, v8::Isolate* isolate);

v8::SharedArrayBuffer*
v8__ValueDeserializer__Delegate__GetSharedArrayBufferFromId(
    v8::ValueDeserializer::Delegate* self, v8::Isolate* isolate,
    uint32_t transfer_id);

v8::WasmModuleObject* v8__ValueDeserializer__Delegate__GetWasmModuleFromId(
    v8::ValueDeserializer::Delegate* self, v8::Isolate* isolate,
    uint32_t clone_id);
}

struct v8__ValueDeserializer__Delegate
    : public v8::ValueDeserializer::Delegate {
  v8::MaybeLocal<v8::Object> ReadHostObject(v8::Isolate* isolate) override {
    return ptr_to_maybe_local(
        v8__ValueDeserializer__Delegate__ReadHostObject(this, isolate));
  }

  v8::MaybeLocal<v8::SharedArrayBuffer> GetSharedArrayBufferFromId(
      v8::Isolate* isolate, uint32_t transfer_id) override {
    return ptr_to_maybe_local(
        v8__ValueDeserializer__Delegate__GetSharedArrayBufferFromId(
            this, isolate, transfer_id));
  }

  v8::MaybeLocal<v8::WasmModuleObject> GetWasmModuleFromId(
      v8::Isolate* isolate, uint32_t clone_id) override {
    return ptr_to_maybe_local(
        v8__ValueDeserializer__Delegate__GetWasmModuleFromId(this, isolate,
                                                             clone_id));
  }
};

extern "C" {
void v8__ValueDeserializer__Delegate__CONSTRUCT(
    uninit_t<v8__ValueDeserializer__Delegate>* buf) {
  static_assert(sizeof(v8__ValueDeserializer__Delegate) == sizeof(size_t),
                "v8__ValueDeserializer__Delegate size mismatch");
  construct_in_place<v8__ValueDeserializer__Delegate>(buf);
}
}

// v8::ValueDeserializer

extern "C" {
void v8__ValueDeserializer__CONSTRUCT(
    uninit_t<v8::ValueDeserializer>* buf, v8::Isolate* isolate,
    const uint8_t* data, size_t size,
    v8::ValueDeserializer::Delegate* delegate) {
  static_assert(sizeof(v8::ValueDeserializer) == sizeof(size_t),
                "v8::ValueDeserializer size mismatch");
  construct_in_place<v8::ValueDeserializer>(buf, isolate, data, size, delegate);
}

void v8__ValueDeserializer__DESTRUCT(v8::ValueDeserializer* self) {
  self->~ValueDeserializer();
}

MaybeBool v8__ValueDeserializer__ReadHeader(v8::ValueDeserializer* self,
                                            v8::Local<v8::Context> context) {
  return maybe_to_maybe_bool(self->ReadHeader(context));
}

v8::Value* v8__ValueDeserializer__ReadValue(v8::ValueDeserializer* self,
                                            v8::Local<v8::Context> context) {
  return maybe_local_to_ptr(self->ReadValue(context));
}

void v8__ValueDeserializer__TransferArrayBuffer(
    v8::ValueDeserializer* self, uint32_t transfer_id,
    v8::Local<v8::ArrayBuffer> array_buffer) {
  self->TransferArrayBuffer(transfer_id, array_buffer);
}

void v8__ValueDeserializer__TransferSharedArrayBuffer(
    v8::ValueDeserializer* self, uint32_t transfer_id,
    v8::Local<v8::SharedArrayBuffer> shared_array_buffer) {
  self->TransferSharedArrayBuffer(transfer_id, shared_array_buffer);
}

void v8__ValueDeserializer__SetSupportsLegacyWireFormat(
    v8::ValueDeserializer* self, bool supports_legacy_wire_format) {
  self->SetSupportsLegacyWireFormat(supports_legacy_wire_format);
}

bool v8__ValueDeserializer__ReadUint32(v8::ValueDeserializer* self,
                                       uint32_t* value) {
  return self->ReadUint32(value);
}

bool v8__ValueDeserializer__ReadUint64(v8::ValueDeserializer* self,
                                       uint64_t* value) {
  return self->ReadUint64(value);
}

bool v8__ValueDeserializer__ReadDouble(v8::ValueDeserializer* self,
                                       double* value) {
  return self->ReadDouble(value);
}

bool v8__ValueDeserializer__ReadRawBytes(v8::ValueDeserializer* self,
                                         size_t length, const void** data) {
  return self->ReadRawBytes(length, data);
}

uint32_t v8__ValueDeserializer__GetWireFormatVersion(
    v8::ValueDeserializer* self) {
  return self->GetWireFormatVersion();
}
}  // extern "C"

// v8::WasmModuleCompilation

extern "C" {

v8::WasmModuleCompilation* v8__WasmModuleCompilation__NEW() {
  return new v8::WasmModuleCompilation();
}

void v8__WasmModuleCompilation__DELETE(v8::WasmModuleCompilation* self) {
  delete self;
}

void v8__WasmModuleCompilation__OnBytesReceived(v8::WasmModuleCompilation* self,
                                                const uint8_t* bytes,
                                                size_t size) {
  self->OnBytesReceived(bytes, size);
}

void v8__WasmModuleCompilation__Finish(
    v8::WasmModuleCompilation* self, v8::Isolate* isolate,
    void (*caching_callback)(v8::WasmStreaming::ModuleCachingInterface&),
    void (*resolution_callback)(void* data, const v8::WasmModuleObject* module,
                                const v8::Value* error),
    void* resolution_data, void (*drop_resolution_data)(void* data)) {
  // Use shared_ptr to reference-count the Rust closure data through
  // std::function's copy semantics. The custom deleter calls back into Rust
  // to drop the boxed closure when the last copy is destroyed.
  auto shared_data = std::shared_ptr<void>(
      resolution_data,
      [drop_resolution_data](void* p) { drop_resolution_data(p); });
  self->Finish(
      isolate, caching_callback,
      [resolution_callback, shared_data](auto result) {
        if (auto* module =
                std::get_if<v8::Local<v8::WasmModuleObject>>(&result)) {
          resolution_callback(shared_data.get(), local_to_ptr(*module),
                              nullptr);
        } else {
          resolution_callback(
              shared_data.get(), nullptr,
              local_to_ptr(std::get<v8::Local<v8::Value>>(result)));
        }
      });
}

void v8__WasmModuleCompilation__Abort(v8::WasmModuleCompilation* self) {
  self->Abort();
}

void v8__WasmModuleCompilation__SetHasCompiledModuleBytes(
    v8::WasmModuleCompilation* self) {
  self->SetHasCompiledModuleBytes();
}

void v8__WasmModuleCompilation__SetMoreFunctionsCanBeSerializedCallback(
    v8::WasmModuleCompilation* self,
    void (*callback)(void* data, v8::CompiledWasmModule* compiled_module),
    void* data, void (*drop_data)(void* data)) {
  auto shared_data =
      std::shared_ptr<void>(data, [drop_data](void* p) { drop_data(p); });
  self->SetMoreFunctionsCanBeSerializedCallback(
      [callback, shared_data](v8::CompiledWasmModule module) {
        auto* heap_module = new v8::CompiledWasmModule(std::move(module));
        callback(shared_data.get(), heap_module);
      });
}

void v8__WasmModuleCompilation__SetUrl(v8::WasmModuleCompilation* self,
                                       const char* url, size_t length) {
  self->SetUrl(url, length);
}

}  // extern "C"

// v8::CompiledWasmModule

extern "C" {
const v8::WasmModuleObject* v8__WasmModuleObject__FromCompiledModule(
    v8::Isolate* isolate, const v8::CompiledWasmModule* compiled_module) {
  return maybe_local_to_ptr(
      v8::WasmModuleObject::FromCompiledModule(isolate, *compiled_module));
}

v8::CompiledWasmModule* v8__WasmModuleObject__GetCompiledModule(
    const v8::WasmModuleObject* self) {
  v8::CompiledWasmModule cwm = ptr_to_local(self)->GetCompiledModule();
  return new v8::CompiledWasmModule(std::move(cwm));
}

const v8::WasmModuleObject* v8__WasmModuleObject__Compile(
    v8::Isolate* isolate, uint8_t* wire_bytes_data, size_t length) {
  v8::MemorySpan<const uint8_t> wire_bytes(wire_bytes_data, length);
  return maybe_local_to_ptr(v8::WasmModuleObject::Compile(isolate, wire_bytes));
}

const uint8_t* v8__CompiledWasmModule__GetWireBytesRef(
    v8::CompiledWasmModule* self, size_t* length) {
  v8::MemorySpan<const uint8_t> span = self->GetWireBytesRef();
  *length = span.size();
  return span.data();
}

const char* v8__CompiledWasmModule__SourceUrl(v8::CompiledWasmModule* self,
                                              size_t* length) {
  const std::string& source_url = self->source_url();
  *length = source_url.size();
  return source_url.data();
}

void v8__CompiledWasmModule__DELETE(v8::CompiledWasmModule* self) {
  delete self;
}
}  // extern "C"

// icu

extern "C" {

size_t icu_get_default_locale(char* output, size_t output_len) {
  const icu::Locale& default_locale = icu::Locale::getDefault();
  icu::CheckedArrayByteSink sink(output, static_cast<uint32_t>(output_len));
  UErrorCode status = U_ZERO_ERROR;
  default_locale.toLanguageTag(sink, status);
  assert(status == U_ZERO_ERROR);
  assert(!sink.Overflowed());
  return sink.NumberOfBytesAppended();
}

void icu_set_default_locale(const char* locale) {
  UErrorCode status = U_ZERO_ERROR;
  icu::Locale::setDefault(icu::Locale(locale), status);
}

size_t icu_get_default_time_zone(char* output, size_t output_len) {
  icu::UnicodeString id;
  std::unique_ptr<icu::TimeZone> tz(icu::TimeZone::createDefault());
  tz->getID(id);
  icu::CheckedArrayByteSink sink(output, static_cast<uint32_t>(output_len));
  id.toUTF8(sink);
  assert(!sink.Overflowed());
  // Deliberately not `NumberOfBytesAppended()`: that counts bytes the sink was
  // asked to write, so on overflow it exceeds `output_len` and the caller would
  // read past the buffer (the assert above is compiled out in release builds).
  return sink.NumberOfBytesWritten();
}

// NOTE: the parameter is deliberately not named `timezone`; glibc's <time.h>
// declares a global with that name and the build uses -Werror,-Wshadow.
bool icu_set_default_time_zone(const char* time_zone_id) {
  // `createTimeZone()` never fails: an id it does not recognize yields the
  // special "unknown" zone (which behaves as GMT). Detect that and report it
  // instead of silently installing GMT.
  std::unique_ptr<icu::TimeZone> tz(icu::TimeZone::createTimeZone(
      icu::UnicodeString::fromUTF8(time_zone_id)));
  icu::UnicodeString id;
  tz->getID(id);
  if (id == icu::UnicodeString(UCAL_UNKNOWN_ZONE_ID, -1, US_INV)) {
    return false;
  }
  // Takes ownership of the created TimeZone and installs it as the process
  // wide default, so ICU (and therefore V8's Date implementation) resolves
  // the given id regardless of how the host reports its time zone.
  icu::TimeZone::adoptDefault(tz.release());
  return true;
}

}  // extern "C"

// v8::PropertyDescriptor

extern "C" {

static_assert(sizeof(v8::PropertyDescriptor) == sizeof(size_t),
              "v8::PropertyDescriptor size mismatch");

void v8__PropertyDescriptor__CONSTRUCT(uninit_t<v8::PropertyDescriptor>* buf) {
  construct_in_place<v8::PropertyDescriptor>(buf);
}

void v8__PropertyDescriptor__CONSTRUCT__Value_Writable(
    uninit_t<v8::PropertyDescriptor>* buf, v8::Local<v8::Value> value,
    bool writable) {
  construct_in_place<v8::PropertyDescriptor>(buf, value, writable);
}

void v8__PropertyDescriptor__CONSTRUCT__Value(
    uninit_t<v8::PropertyDescriptor>* buf, v8::Local<v8::Value> value) {
  construct_in_place<v8::PropertyDescriptor>(buf, value);
}

void v8__PropertyDescriptor__CONSTRUCT__Get_Set(
    uninit_t<v8::PropertyDescriptor>* buf, v8::Local<v8::Value> get,
    v8::Local<v8::Value> set) {
  construct_in_place<v8::PropertyDescriptor>(buf, get, set);
}

void v8__PropertyDescriptor__DESTRUCT(v8::PropertyDescriptor* self) {
  self->~PropertyDescriptor();
}

bool v8__PropertyDescriptor__configurable(const v8::PropertyDescriptor* self) {
  return self->configurable();
}

bool v8__PropertyDescriptor__enumerable(const v8::PropertyDescriptor* self) {
  return self->enumerable();
}

bool v8__PropertyDescriptor__writable(const v8::PropertyDescriptor* self) {
  return self->writable();
}

const v8::Value* v8__PropertyDescriptor__value(
    const v8::PropertyDescriptor* self) {
  return local_to_ptr(self->value());
}

const v8::Value* v8__PropertyDescriptor__get(
    const v8::PropertyDescriptor* self) {
  return local_to_ptr(self->get());
}

const v8::Value* v8__PropertyDescriptor__set(
    const v8::PropertyDescriptor* self) {
  return local_to_ptr(self->set());
}

bool v8__PropertyDescriptor__has_configurable(
    const v8::PropertyDescriptor* self) {
  return self->has_configurable();
}

bool v8__PropertyDescriptor__has_enumerable(
    const v8::PropertyDescriptor* self) {
  return self->has_enumerable();
}

bool v8__PropertyDescriptor__has_writable(const v8::PropertyDescriptor* self) {
  return self->has_writable();
}

bool v8__PropertyDescriptor__has_value(const v8::PropertyDescriptor* self) {
  return self->has_value();
}

bool v8__PropertyDescriptor__has_get(const v8::PropertyDescriptor* self) {
  return self->has_get();
}

bool v8__PropertyDescriptor__has_set(const v8::PropertyDescriptor* self) {
  return self->has_set();
}

void v8__PropertyDescriptor__set_enumerable(v8::PropertyDescriptor* self,
                                            bool enumurable) {
  self->set_enumerable(enumurable);
}

void v8__PropertyDescriptor__set_configurable(v8::PropertyDescriptor* self,
                                              bool configurable) {
  self->set_configurable(configurable);
}

}  // extern "C"

// cppgc

extern "C" {

void rusty_v8_RustObj_trace(const RustObj*, cppgc::Visitor*);
const char* rusty_v8_RustObj_get_name(const RustObj*);
void rusty_v8_RustObj_drop(RustObj*);

RustObj::~RustObj() { rusty_v8_RustObj_drop(this); }

void RustObj::Trace(cppgc::Visitor* visitor) const {
  rusty_v8_RustObj_trace(this, visitor);
}

const char* RustObj::GetHumanReadableName() const {
  return rusty_v8_RustObj_get_name(this);
}

RustObj* v8__Object__Unwrap(v8::Isolate* isolate, const v8::Object& wrapper,
                            v8::CppHeapPointerTag tag) {
  v8::CppHeapPointerTagRange tag_range(tag, tag);
  return static_cast<RustObj*>(
      v8::Object::Unwrap(isolate, ptr_to_local(&wrapper), tag_range));
}

void v8__Object__Wrap(v8::Isolate* isolate, const v8::Object& wrapper,
                      RustObj* value, v8::CppHeapPointerTag tag) {
  v8::Object::Wrap(isolate, ptr_to_local(&wrapper), value, tag);
}

v8::CppHeap* v8__Isolate__GetCppHeap(v8::Isolate* isolate) {
  return isolate->GetCppHeap();
}

void cppgc__initialize_process(v8::Platform* platform) {
  cppgc::InitializeProcess(platform->GetPageAllocator());
}

void cppgc__shutdown_process() { cppgc::ShutdownProcess(); }

v8::CppHeap* v8__CppHeap__Create(v8::Platform* platform,
                                 cppgc::Heap::MarkingType marking_support,
                                 cppgc::Heap::SweepingType sweeping_support) {
  v8::CppHeapCreateParams params{{}};
  params.marking_support = marking_support;
  params.sweeping_support = sweeping_support;
  std::unique_ptr<v8::CppHeap> heap = v8::CppHeap::Create(platform, params);
  return heap.release();
}

void v8__CppHeap__DELETE(v8::CppHeap* self) { delete self; }

void cppgc__heap__enable_detached_garbage_collections_for_testing(
    v8::CppHeap* heap) {
  heap->EnableDetachedGarbageCollectionsForTesting();
}

void cppgc__heap__collect_garbage_for_testing(
    v8::CppHeap* heap, cppgc::EmbedderStackState stack_state) {
  heap->CollectGarbageForTesting(stack_state);
}

class alignas(16) RustObjButAlign16 : public RustObj {};

RustObj* cppgc__make_garbage_collectable(v8::CppHeap* heap, size_t size,
                                         size_t alignment) {
  if (alignment <= 8) {
    return cppgc::MakeGarbageCollected<RustObj>(heap->GetAllocationHandle(),
                                                cppgc::AdditionalBytes(size));
  }
  if (alignment <= 16) {
    return cppgc::MakeGarbageCollected<RustObjButAlign16>(
        heap->GetAllocationHandle(), cppgc::AdditionalBytes(size));
  }
  return nullptr;
}

void cppgc__Visitor__Trace__Member(cppgc::Visitor* visitor,
                                   cppgc::Member<RustObj>* member) {
  visitor->Trace(*member);
}

void cppgc__Visitor__Trace__WeakMember(cppgc::Visitor* visitor,
                                       cppgc::WeakMember<RustObj>* member) {
  visitor->Trace(*member);
}

void cppgc__Visitor__Trace__TracedReference(
    cppgc::Visitor* visitor, v8::TracedReference<v8::Data>* ref) {
  visitor->Trace(*ref);
}

void cppgc__Member__CONSTRUCT(uninit_t<cppgc::Member<RustObj>>* buf,
                              RustObj* other) {
  construct_in_place<cppgc::Member<RustObj>>(buf, other);
}

void cppgc__Member__DESTRUCT(cppgc::Member<RustObj>* self) {
  self->~BasicMember();
}

RustObj* cppgc__Member__Get(cppgc::Member<RustObj>* member) {
  return member->Get();
}

void cppgc__Member__Assign(cppgc::Member<RustObj>* member, RustObj* other) {
  member->operator=(other);
}

void cppgc__WeakMember__CONSTRUCT(uninit_t<cppgc::WeakMember<RustObj>>* buf,
                                  RustObj* other) {
  construct_in_place<cppgc::WeakMember<RustObj>>(buf, other);
}

void cppgc__WeakMember__DESTRUCT(cppgc::WeakMember<RustObj>* self) {
  self->~BasicMember();
}

RustObj* cppgc__WeakMember__Get(cppgc::WeakMember<RustObj>* member) {
  return member->Get();
}

void cppgc__WeakMember__Assign(cppgc::WeakMember<RustObj>* member,
                               RustObj* other) {
  member->operator=(other);
}

cppgc::Persistent<RustObj>* cppgc__Persistent__CONSTRUCT(RustObj* obj) {
  return new cppgc::Persistent<RustObj>(obj);
}

void cppgc__Persistent__DESTRUCT(cppgc::Persistent<RustObj>* self) {
  delete self;
}

void cppgc__Persistent__Assign(cppgc::Persistent<RustObj>* self, RustObj* ptr) {
  self->operator=(ptr);
}

RustObj* cppgc__Persistent__Get(cppgc::Persistent<RustObj>* self) {
  return self->Get();
}

cppgc::WeakPersistent<RustObj>* cppgc__WeakPersistent__CONSTRUCT(RustObj* obj) {
  return new cppgc::WeakPersistent<RustObj>(obj);
}

void cppgc__WeakPersistent__DESTRUCT(cppgc::WeakPersistent<RustObj>* self) {
  delete self;
}

void cppgc__WeakPersistent__Assign(cppgc::WeakPersistent<RustObj>* self,
                                   RustObj* ptr) {
  self->operator=(ptr);
}

RustObj* cppgc__WeakPersistent__Get(cppgc::WeakPersistent<RustObj>* self) {
  return self->Get();
}

}  // extern "C"

// =============================================================================
// simdutf bindings
// =============================================================================

#include "third_party/simdutf/simdutf.h"

struct simdutf__result {
  int error;
  size_t count;
};

static simdutf__result to_ffi_result(simdutf::result r) {
  return {static_cast<int>(r.error), r.count};
}

extern "C" {

// --- Validation ---

bool simdutf__validate_utf8(const char* buf, size_t len) {
  return simdutf::validate_utf8(buf, len);
}

simdutf__result simdutf__validate_utf8_with_errors(const char* buf,
                                                   size_t len) {
  return to_ffi_result(simdutf::validate_utf8_with_errors(buf, len));
}

bool simdutf__validate_ascii(const char* buf, size_t len) {
  return simdutf::validate_ascii(buf, len);
}

simdutf__result simdutf__validate_ascii_with_errors(const char* buf,
                                                    size_t len) {
  return to_ffi_result(simdutf::validate_ascii_with_errors(buf, len));
}

bool simdutf__validate_utf16le(const char16_t* buf, size_t len) {
  return simdutf::validate_utf16le(buf, len);
}

simdutf__result simdutf__validate_utf16le_with_errors(const char16_t* buf,
                                                      size_t len) {
  return to_ffi_result(simdutf::validate_utf16le_with_errors(buf, len));
}

bool simdutf__validate_utf16be(const char16_t* buf, size_t len) {
  return simdutf::validate_utf16be(buf, len);
}

simdutf__result simdutf__validate_utf16be_with_errors(const char16_t* buf,
                                                      size_t len) {
  return to_ffi_result(simdutf::validate_utf16be_with_errors(buf, len));
}

bool simdutf__validate_utf32(const char32_t* buf, size_t len) {
  return simdutf::validate_utf32(buf, len);
}

simdutf__result simdutf__validate_utf32_with_errors(const char32_t* buf,
                                                    size_t len) {
  return to_ffi_result(simdutf::validate_utf32_with_errors(buf, len));
}

// --- Conversion: UTF-8 <-> UTF-16LE ---

size_t simdutf__convert_utf8_to_utf16le(const char* input, size_t length,
                                        char16_t* output) {
  return simdutf::convert_utf8_to_utf16le(input, length, output);
}

simdutf__result simdutf__convert_utf8_to_utf16le_with_errors(const char* input,
                                                             size_t length,
                                                             char16_t* output) {
  return to_ffi_result(
      simdutf::convert_utf8_to_utf16le_with_errors(input, length, output));
}

size_t simdutf__convert_valid_utf8_to_utf16le(const char* input, size_t length,
                                              char16_t* output) {
  return simdutf::convert_valid_utf8_to_utf16le(input, length, output);
}

size_t simdutf__convert_utf16le_to_utf8(const char16_t* input, size_t length,
                                        char* output) {
  return simdutf::convert_utf16le_to_utf8(input, length, output);
}

simdutf__result simdutf__convert_utf16le_to_utf8_with_errors(
    const char16_t* input, size_t length, char* output) {
  return to_ffi_result(
      simdutf::convert_utf16le_to_utf8_with_errors(input, length, output));
}

size_t simdutf__convert_valid_utf16le_to_utf8(const char16_t* input,
                                              size_t length, char* output) {
  return simdutf::convert_valid_utf16le_to_utf8(input, length, output);
}

// --- Conversion: UTF-8 <-> UTF-16BE ---

size_t simdutf__convert_utf8_to_utf16be(const char* input, size_t length,
                                        char16_t* output) {
  return simdutf::convert_utf8_to_utf16be(input, length, output);
}

size_t simdutf__convert_utf16be_to_utf8(const char16_t* input, size_t length,
                                        char* output) {
  return simdutf::convert_utf16be_to_utf8(input, length, output);
}

// --- Conversion: UTF-8 <-> Latin-1 ---

size_t simdutf__convert_utf8_to_latin1(const char* input, size_t length,
                                       char* output) {
  return simdutf::convert_utf8_to_latin1(input, length, output);
}

simdutf__result simdutf__convert_utf8_to_latin1_with_errors(const char* input,
                                                            size_t length,
                                                            char* output) {
  return to_ffi_result(
      simdutf::convert_utf8_to_latin1_with_errors(input, length, output));
}

size_t simdutf__convert_valid_utf8_to_latin1(const char* input, size_t length,
                                             char* output) {
  return simdutf::convert_valid_utf8_to_latin1(input, length, output);
}

size_t simdutf__convert_latin1_to_utf8(const char* input, size_t length,
                                       char* output) {
  return simdutf::convert_latin1_to_utf8(input, length, output);
}

// --- Conversion: Latin-1 <-> UTF-16LE ---

size_t simdutf__convert_latin1_to_utf16le(const char* input, size_t length,
                                          char16_t* output) {
  return simdutf::convert_latin1_to_utf16le(input, length, output);
}

size_t simdutf__convert_utf16le_to_latin1(const char16_t* input, size_t length,
                                          char* output) {
  return simdutf::convert_utf16le_to_latin1(input, length, output);
}

// --- Conversion: UTF-8 <-> UTF-32 ---

size_t simdutf__convert_utf8_to_utf32(const char* input, size_t length,
                                      char32_t* output) {
  return simdutf::convert_utf8_to_utf32(input, length, output);
}

size_t simdutf__convert_utf32_to_utf8(const char32_t* input, size_t length,
                                      char* output) {
  return simdutf::convert_utf32_to_utf8(input, length, output);
}

// --- Length calculation ---

size_t simdutf__utf8_length_from_utf16le(const char16_t* input, size_t length) {
  return simdutf::utf8_length_from_utf16le(input, length);
}

size_t simdutf__utf8_length_from_utf16be(const char16_t* input, size_t length) {
  return simdutf::utf8_length_from_utf16be(input, length);
}

size_t simdutf__utf16_length_from_utf8(const char* input, size_t length) {
  return simdutf::utf16_length_from_utf8(input, length);
}

size_t simdutf__utf8_length_from_latin1(const char* input, size_t length) {
  return simdutf::utf8_length_from_latin1(input, length);
}

size_t simdutf__latin1_length_from_utf8(const char* input, size_t length) {
  return simdutf::latin1_length_from_utf8(input, length);
}

size_t simdutf__utf32_length_from_utf8(const char* input, size_t length) {
  return simdutf::utf32_length_from_utf8(input, length);
}

size_t simdutf__utf8_length_from_utf32(const char32_t* input, size_t length) {
  return simdutf::utf8_length_from_utf32(input, length);
}

size_t simdutf__utf16_length_from_utf32(const char32_t* input, size_t length) {
  return simdutf::utf16_length_from_utf32(input, length);
}

size_t simdutf__utf32_length_from_utf16le(const char16_t* input,
                                          size_t length) {
  return simdutf::utf32_length_from_utf16le(input, length);
}

// --- Counting ---

size_t simdutf__count_utf8(const char* input, size_t length) {
  return simdutf::count_utf8(input, length);
}

size_t simdutf__count_utf16le(const char16_t* input, size_t length) {
  return simdutf::count_utf16le(input, length);
}

size_t simdutf__count_utf16be(const char16_t* input, size_t length) {
  return simdutf::count_utf16be(input, length);
}

// --- Encoding detection ---

int simdutf__detect_encodings(const char* input, size_t length) {
  return simdutf::detect_encodings(input, length);
}

// --- Base64 ---

size_t simdutf__maximal_binary_length_from_base64(const char* input,
                                                  size_t length) {
  return simdutf::maximal_binary_length_from_base64(input, length);
}

simdutf__result simdutf__base64_to_binary(const char* input, size_t length,
                                          char* output, uint64_t options,
                                          uint64_t last_chunk_options) {
  return to_ffi_result(simdutf::base64_to_binary(
      input, length, output, static_cast<simdutf::base64_options>(options),
      static_cast<simdutf::last_chunk_handling_options>(last_chunk_options)));
}

size_t simdutf__base64_length_from_binary(size_t length, uint64_t options) {
  return simdutf::base64_length_from_binary(
      length, static_cast<simdutf::base64_options>(options));
}

size_t simdutf__binary_to_base64(const char* input, size_t length, char* output,
                                 uint64_t options) {
  return simdutf::binary_to_base64(
      input, length, output, static_cast<simdutf::base64_options>(options));
}

}  // extern "C"
