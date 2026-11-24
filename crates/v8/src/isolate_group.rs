// Copyright 2018-2025 the Deno authors. All rights reserved. MIT license.

use crate::Isolate;
use crate::isolate::RealIsolate;
use crate::support::Opaque;
use std::fmt;

#[repr(C)]
struct InternalIsolateGroup(Opaque);

unsafe extern "C" {
  fn v8__IsolateGroup__GetDefault() -> *const InternalIsolateGroup;
  fn v8__IsolateGroup__CanCreateNewGroups() -> bool;
  fn v8__IsolateGroup__Create() -> *const InternalIsolateGroup;
  fn v8__Isolate__GetGroup(
    isolate: *const RealIsolate,
  ) -> *const InternalIsolateGroup;
  fn v8__IsolateGroup__CLONE(
    this: *const IsolateGroup,
  ) -> *const InternalIsolateGroup;
  fn v8__IsolateGroup__DESTRUCT(this: *mut IsolateGroup);
  fn v8__IsolateGroup__EQ(
    this: *const IsolateGroup,
    other: *const IsolateGroup,
  ) -> bool;
}

/// The set of V8 isolates in a process is partitioned into groups. Each group
/// has its own sandbox (if V8 was configured with support for the sandbox) and
/// pointer-compression cage (if configured with pointer compression).
///
/// By default, all isolates are placed in the same group. This is the most
/// efficient configuration in terms of speed and memory use. However, with
/// pointer compression enabled, total heap usage of isolates in a group cannot
/// exceed 4 GB, not counting array buffers and other off-heap storage. Using
/// multiple isolate groups can allow embedders to allocate more than 4GB of
/// objects with pointer compression enabled, if the embedder's use case can
/// span multiple isolates.
///
/// Creating an isolate group reserves a range of virtual memory addresses. A
/// group's memory mapping will be released when the last isolate in the group
/// is disposed, and there are no more live `IsolateGroup` objects that refer to
/// it.
///
/// Note that isolate groups are reference counted, and this type is a reference
/// to one. Cloning it produces another reference to the same group; comparing
/// two values tells you whether they refer to the same group.
///
/// Note that it's not going to be possible to pass shared JS objects across an
/// `IsolateGroup` boundary.
///
/// # Lifetime
///
/// An isolate holds its own reference to the group it was created in, so the
/// group outlives its isolates whether or not you keep this handle around.
///
/// Do not, however, hold one of these across [`crate::V8::dispose`]. Tearing
/// down V8 asserts that the only remaining reference to the default group is
/// V8's own, and a live handle here makes that assertion fail, aborting the
/// process. Dropping a handle *after* `dispose` is equally unsupported, since
/// releasing the last reference reaches for a page allocator that is gone by
/// then. Drop every `IsolateGroup` before shutting V8 down.
///
/// # Build configuration
///
/// Creating groups beyond the default one requires V8 to be built with pointer
/// compression in multi-cage mode, i.e. `v8_enable_pointer_compression=true`
/// together with `v8_enable_pointer_compression_shared_cage=false`. That
/// configuration is mutually exclusive with the V8 sandbox. Use
/// [`IsolateGroup::can_create_new_groups`] to find out whether the V8 this
/// crate was linked against supports it.
#[repr(C)]
pub struct IsolateGroup(*const InternalIsolateGroup);

// SAFETY: The group's reference count is a `std::atomic<int>`, so acquiring and
// releasing references (which is all cloning and dropping do) is thread safe,
// and V8 supports isolates belonging to one group living on different threads.
unsafe impl Send for IsolateGroup {}
// SAFETY: `&IsolateGroup` only ever reads the pointer, either to compare it or
// to hand it to V8 when creating an isolate.
unsafe impl Sync for IsolateGroup {}

impl IsolateGroup {
  /// Returns true if new isolate groups can be created at run-time, or false if
  /// all isolates must be in the same group.
  ///
  /// This is a property of how V8 was built, not of the running process, so the
  /// answer never changes over the lifetime of the program.
  pub fn can_create_new_groups() -> bool {
    unsafe { v8__IsolateGroup__CanCreateNewGroups() }
  }

  /// Gets the default isolate group. If this V8's build configuration only
  /// supports a single group, this is a reference to that single group.
  /// Otherwise this is a group like any other, distinguished only in that it is
  /// the first group.
  ///
  /// # Panics
  ///
  /// Panics if V8 has not been initialized. V8 only creates the default group
  /// during initialization, and would dereference a null pointer before then.
  pub fn get_default() -> Self {
    crate::V8::assert_initialized();
    IsolateGroup(unsafe { v8__IsolateGroup__GetDefault() })
  }

  /// Creates a new isolate group.
  ///
  /// # Panics
  ///
  /// Panics if this V8's build configuration only supports a single group.
  /// Check [`IsolateGroup::can_create_new_groups`] first, or use
  /// [`IsolateGroup::try_create`], which returns `None` instead. Also panics
  /// if V8 has not been initialized.
  pub fn create() -> Self {
    Self::try_create().expect(
      "V8 was not built with support for multiple isolate groups; this \
       requires v8_enable_pointer_compression=true together with \
       v8_enable_pointer_compression_shared_cage=false",
    )
  }

  /// Creates a new isolate group, or returns `None` if this V8's build
  /// configuration only supports a single group.
  ///
  /// # Panics
  ///
  /// Panics if V8 has not been initialized; creating a group needs the
  /// platform's virtual address space.
  pub fn try_create() -> Option<Self> {
    crate::V8::assert_initialized();
    // V8 aborts the process rather than failing gracefully, so the check has to
    // happen before the call rather than on its result.
    if !Self::can_create_new_groups() {
      return None;
    }
    Some(IsolateGroup(unsafe { v8__IsolateGroup__Create() }))
  }
}

impl Isolate {
  /// Returns the [`IsolateGroup`] this isolate belongs to.
  pub fn get_group(&self) -> IsolateGroup {
    IsolateGroup(unsafe { v8__Isolate__GetGroup(self.as_real_ptr()) })
  }
}

impl Default for IsolateGroup {
  fn default() -> Self {
    IsolateGroup::get_default()
  }
}

impl Clone for IsolateGroup {
  fn clone(&self) -> Self {
    IsolateGroup(unsafe { v8__IsolateGroup__CLONE(self) })
  }
}

impl Drop for IsolateGroup {
  fn drop(&mut self) {
    unsafe { v8__IsolateGroup__DESTRUCT(self) }
  }
}

impl Eq for IsolateGroup {}

impl PartialEq for IsolateGroup {
  fn eq(&self, other: &Self) -> bool {
    unsafe { v8__IsolateGroup__EQ(self, other) }
  }
}

impl fmt::Debug for IsolateGroup {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.debug_tuple("IsolateGroup").field(&self.0).finish()
  }
}
