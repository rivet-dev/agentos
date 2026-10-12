//! Native heap maintenance for memory freed by V8 and other C/C++ code.

/// Return free glibc heap pages to the OS.
///
/// V8 isolates and other native code allocate through glibc malloc, which keeps
/// freed arena memory mapped after an isolate is destroyed. Returns whether
/// any memory was released. On other platforms and C libraries this is a no-op.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn release_free_native_heap() -> bool {
    // SAFETY: `malloc_trim` has no preconditions; it takes the allocator's own
    // arena locks and only releases pages that are already free.
    unsafe { libc::malloc_trim(0) != 0 }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn release_free_native_heap() -> bool {
    false
}
