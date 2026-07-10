use crate::array_buffer;
use crate::array_buffer::Allocator as ArrayBufferAllocator;
use crate::cppgc::Heap;
use crate::support::Allocated;
use crate::support::Allocation;
use crate::support::Opaque;
use crate::support::SharedPtr;
use crate::support::UniqueRef;
use crate::support::char;
use crate::support::int;
use crate::support::intptr_t;

use std::any::Any;
use std::convert::TryFrom;
use std::iter::once;
use std::mem::MaybeUninit;
use std::mem::size_of;
use std::ptr::null;

/// Should return a pointer to memory that persists for the lifetime of the
/// isolate.
pub type CounterLookupCallback =
  unsafe extern "C" fn(name: *const char) -> *mut i32;

/// Initial configuration parameters for a new Isolate.
#[must_use]
#[derive(Debug, Default)]
pub struct CreateParams {
  raw: raw::CreateParams,
  allocations: CreateParamAllocations,
}

impl CreateParams {
  /// Enables the host application to provide a mechanism for recording
  /// statistics counters.
  pub fn counter_lookup_callback(
    mut self,
    callback: CounterLookupCallback,
  ) -> Self {
    self.raw.counter_lookup_callback = Some(callback);
    self
  }

  /// Explicitly specify a startup snapshot blob.
  pub fn snapshot_blob(mut self, data: impl Allocated<[u8]>) -> Self {
    let data = Allocation::of(data);
    let header = Allocation::of(raw::StartupData::boxed_header(&data));
    self.raw.snapshot_blob = &*header;
    self.allocations.snapshot_blob_data = Some(data);
    self.allocations.snapshot_blob_header = Some(header);
    self
  }

  /// The ArrayBuffer::ArrayBufferAllocator to use for allocating and freeing
  /// the backing store of ArrayBuffers.
  pub fn array_buffer_allocator(
    mut self,
    array_buffer_allocator: impl Into<SharedPtr<ArrayBufferAllocator>>,
  ) -> Self {
    self.raw.array_buffer_allocator_shared = array_buffer_allocator.into();
    self
  }

  /// Check if `array_buffer_allocator` has already been called. Useful to some
  /// embedders that might want to set an allocator but not overwrite if one
  /// was already set by a user.
  pub fn has_set_array_buffer_allocator(&self) -> bool {
    !self.raw.array_buffer_allocator_shared.is_null()
  }

  /// Specifies an optional nullptr-terminated array of raw addresses in the
  /// embedder that V8 can match against during serialization and use for
  /// deserialization. This array and its content must stay valid for the
  /// entire lifetime of the isolate.
  pub fn external_references(
    mut self,
    ext_refs: impl Allocated<[intptr_t]>,
  ) -> Self {
    let last_non_null = ext_refs
      .iter()
      .cloned()
      .enumerate()
      .rev()
      .find_map(|(idx, value)| if value != 0 { Some(idx) } else { None });
    let first_null = ext_refs
      .iter()
      .cloned()
      .enumerate()
      .find_map(|(idx, value)| if value == 0 { Some(idx) } else { None });
    match (last_non_null, first_null) {
      (None, _) => {
        // Empty list.
        self.raw.external_references = null();
        self.allocations.external_references = None;
      }
      (_, None) => {
        // List does not have null terminator. Make a copy and add it.
        let ext_refs =
          ext_refs.iter().cloned().chain(once(0)).collect::<Vec<_>>();
        let ext_refs = Allocation::of(ext_refs);
        self.raw.external_references = &ext_refs[0];
        self.allocations.external_references = Some(ext_refs);
      }
      (Some(idx1), Some(idx2)) if idx1 + 1 == idx2 => {
        // List is properly null terminated, we'll use it as-is.
        let ext_refs = Allocation::of(ext_refs);
        self.raw.external_references = &ext_refs[0];
        self.allocations.external_references = Some(ext_refs);
      }
      _ => panic!("unexpected null pointer in external references list"),
    }
    self
  }

  /// Whether calling Atomics.wait (a function that may block) is allowed in
  /// this isolate. This can also be configured via SetAllowAtomicsWait.
  pub fn allow_atomics_wait(mut self, value: bool) -> Self {
    self.raw.allow_atomics_wait = value;
    self
  }

  /// The following parameters describe the offsets for addressing type info
  /// for wrapped API objects and are used by the fast C API
  /// (for details see v8-fast-api-calls.h).
  pub fn embedder_wrapper_type_info_offsets(
    mut self,
    embedder_wrapper_type_index: int,
    embedder_wrapper_object_index: int,
  ) -> Self {
    self.raw.embedder_wrapper_type_index = embedder_wrapper_type_index;
    self.raw.embedder_wrapper_object_index = embedder_wrapper_object_index;
    self
  }

  /// Configures the constraints with reasonable default values based on the
  /// provided lower and upper bounds.
  ///
  /// By default V8 starts with a small heap and dynamically grows it to match
  /// the set of live objects. This may lead to ineffective garbage collections
  /// at startup if the live set is large. Setting the initial heap size avoids
  /// such garbage collections. Note that this does not affect young generation
  /// garbage collections.
  ///
  /// When the heap size approaches `max`, V8 will perform series of
  /// garbage collections and invoke the
  /// [NearHeapLimitCallback](struct.Isolate.html#method.add_near_heap_limit_callback).
  /// If the garbage collections do not help and the callback does not
  /// increase the limit, then V8 will crash with V8::FatalProcessOutOfMemory.
  ///
  /// The heap size includes both the young and the old generation.
  ///
  /// # Arguments
  ///
  /// * `initial` - The initial heap size or zero in bytes
  /// * `max` - The hard limit for the heap size in bytes
  pub fn heap_limits(mut self, initial: usize, max: usize) -> Self {
    self
      .raw
      .constraints
      .configure_defaults_from_heap_size(initial, max);
    self
  }

  /// Configures the constraints with reasonable default values based on the capabilities
  /// of the current device the VM is running on.
  ///
  /// By default V8 starts with a small heap and dynamically grows it to match
  /// the set of live objects. This may lead to ineffective garbage collections
  /// at startup if the live set is large. Setting the initial heap size avoids
  /// such garbage collections. Note that this does not affect young generation
  /// garbage collections.
  ///
  /// When the heap size approaches its maximum, V8 will perform series of
  /// garbage collections and invoke the
  /// [NearHeapLimitCallback](struct.Isolate.html#method.add_near_heap_limit_callback).
  /// If the garbage collections do not help and the callback does not
  /// increase the limit, then V8 will crash with V8::FatalProcessOutOfMemory.
  ///
  /// # Arguments
  ///
  /// * `physical_memory` - The total amount of physical memory on the current device, in bytes.
  /// * `virtual_memory_limit` - The amount of virtual memory on the current device, in bytes, or zero, if there is no limit.
  pub fn heap_limits_from_system_memory(
    mut self,
    physical_memory: u64,
    virtual_memory_limit: u64,
  ) -> Self {
    self
      .raw
      .constraints
      .configure_defaults(physical_memory, virtual_memory_limit);
    self
  }

  /// A CppHeap used to construct the Isolate. V8 takes ownership of the
  /// CppHeap passed this way.
  pub fn cpp_heap(mut self, heap: UniqueRef<Heap>) -> Self {
    self.raw.cpp_heap = heap.into_raw();
    self
  }

  pub(crate) fn finalize(mut self) -> (raw::CreateParams, Box<dyn Any>) {
    if self.raw.array_buffer_allocator_shared.is_null() {
      self = self.array_buffer_allocator(array_buffer::new_default_allocator());
    }
    let Self { raw, allocations } = self;
    (raw, Box::new(allocations))
  }
}

#[derive(Debug, Default)]
struct CreateParamAllocations {
  // Owner of the snapshot data buffer itself.
  snapshot_blob_data: Option<Allocation<[u8]>>,
  // Owns `struct StartupData` which contains just the (ptr, len) tuple in V8's
  // preferred format. We have to heap allocate this because we need to put a
  // stable pointer to it in `CreateParams`.
  snapshot_blob_header: Option<Allocation<raw::StartupData>>,
  external_references: Option<Allocation<[intptr_t]>>,
}

#[test]
fn create_param_defaults() {
  let params = CreateParams::default();
  assert_eq!(params.raw.embedder_wrapper_type_index, -1);
  assert_eq!(params.raw.embedder_wrapper_object_index, -1);
  assert!(params.raw.allow_atomics_wait);
}

pub(crate) mod raw {
  use super::*;

  #[repr(C)]
  #[derive(Debug)]
  pub(crate) struct CreateParams {
    pub code_event_handler: *const Opaque, // JitCodeEventHandler
    pub constraints: ResourceConstraints,
    pub snapshot_blob: *const StartupData,
    pub counter_lookup_callback: Option<CounterLookupCallback>,
    pub create_histogram_callback: *const Opaque, // CreateHistogramCallback
    pub add_histogram_sample_callback: *const Opaque, // AddHistogramSampleCallback
    pub array_buffer_allocator: *mut ArrayBufferAllocator,
    pub array_buffer_allocator_shared: SharedPtr<ArrayBufferAllocator>,
    pub external_references: *const intptr_t,
    pub allow_atomics_wait: bool,
    pub embedder_wrapper_type_index: int,
    pub embedder_wrapper_object_index: int,
    _fatal_error_handler: *const Opaque, // FatalErrorCallback
    _oom_error_handler: *const Opaque,   // OOMErrorCallback
    pub cpp_heap: *const Heap,
  }

  unsafe extern "C" {
    fn v8__Isolate__CreateParams__CONSTRUCT(
      buf: *mut MaybeUninit<CreateParams>,
    );
    fn v8__Isolate__CreateParams__SIZEOF() -> usize;
  }

  impl Default for CreateParams {
    fn default() -> Self {
      let size = unsafe { v8__Isolate__CreateParams__SIZEOF() };
      assert!(size <= size_of::<Self>());
      let mut buf = MaybeUninit::<Self>::uninit();
      unsafe { v8__Isolate__CreateParams__CONSTRUCT(&mut buf) };
      unsafe { buf.assume_init() }
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(crate) struct StartupData {
    pub data: *const char,
    pub raw_size: int,
  }

  impl StartupData {
    pub(crate) fn boxed_header(data: &Allocation<[u8]>) -> Box<Self> {
      Box::new(Self {
        data: &data[0] as *const _ as *const char,
        raw_size: int::try_from(data.len()).unwrap(),
      })
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(crate) struct ResourceConstraints {
    code_range_size_: usize,
    max_old_generation_size_: usize,
    max_young_generation_size_: usize,
    initial_old_generation_size_: usize,
    initial_young_generation_size_: usize,
    stack_limit_: *mut u32,
  }

  unsafe extern "C" {
    fn v8__ResourceConstraints__ConfigureDefaultsFromHeapSize(
      constraints: *mut ResourceConstraints,
      initial_heap_size_in_bytes: usize,
      maximum_heap_size_in_bytes: usize,
    );
    fn v8__ResourceConstraints__ConfigureDefaults(
      constraints: *mut ResourceConstraints,
      physical_memory: u64,
      virtual_memory_limit: u64,
    );
  }

  impl ResourceConstraints {
    pub fn configure_defaults_from_heap_size(
      &mut self,
      initial_heap_size_in_bytes: usize,
      maximum_heap_size_in_bytes: usize,
    ) {
      unsafe {
        v8__ResourceConstraints__ConfigureDefaultsFromHeapSize(
          self,
          initial_heap_size_in_bytes,
          maximum_heap_size_in_bytes,
        );
      };
    }

    pub fn configure_defaults(
      &mut self,
      physical_memory: u64,
      virtual_memory_limit: u64,
    ) {
      unsafe {
        v8__ResourceConstraints__ConfigureDefaults(
          self,
          physical_memory,
          virtual_memory_limit,
        );
      }
    }
  }
}
