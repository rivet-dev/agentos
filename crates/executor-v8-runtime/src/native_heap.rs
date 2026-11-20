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

/// Make glibc return freed memory to the OS instead of keeping it in arenas.
///
/// Every V8 session thread allocates from its own glibc arena. By default glibc
/// raises its mmap threshold each time a large mmapped block is freed, so later
/// blocks of that size land in an arena, and arena memory above the top chunk
/// stays mapped until it passes a 128 KiB-to-32 MiB moving trim threshold. A
/// sidecar that ran many VMs therefore keeps hundreds of MiB after they stop.
/// Fixed thresholds stop both effects: blocks of 4 MiB or more always use
/// `mmap` (unmapped on free), and arena tops above 1 MiB go back to the OS.
/// Lower thresholds return a little more memory per live VM but add
/// measurable page-fault and syscall cost to module loading, which allocates
/// and frees many sub-megabyte blocks.
///
/// Call once at process start, before other threads exist. An operator's
/// `MALLOC_TRIM_THRESHOLD_` or `MALLOC_MMAP_THRESHOLD_` stays in effect.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn configure_native_heap() {
    const TRIM_THRESHOLD_BYTES: libc::c_int = 1024 * 1024;
    const MMAP_THRESHOLD_BYTES: libc::c_int = 4 * 1024 * 1024;
    for (param, value, env, name) in [
        (
            libc::M_TRIM_THRESHOLD,
            TRIM_THRESHOLD_BYTES,
            "MALLOC_TRIM_THRESHOLD_",
            "M_TRIM_THRESHOLD",
        ),
        (
            libc::M_MMAP_THRESHOLD,
            MMAP_THRESHOLD_BYTES,
            "MALLOC_MMAP_THRESHOLD_",
            "M_MMAP_THRESHOLD",
        ),
    ] {
        if std::env::var_os(env).is_some() {
            continue;
        }
        // SAFETY: `mallopt` only updates allocator tunables under its own lock.
        if unsafe { libc::mallopt(param, value) } != 1 {
            tracing::warn!(
                param = name,
                value,
                "ERR_AGENTOS_NATIVE_HEAP_CONFIG: glibc rejected mallopt; freed memory may stay mapped"
            );
        }
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn configure_native_heap() {}
