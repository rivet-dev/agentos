# std patch: ExitStatusExt for wasm32-wasip1

codex-core's `synthetic_exit_status` constructs an `ExitStatus` from a raw code via
`std::os::wasi::process::ExitStatusExt::from_raw`, which wasi std lacks. Add it
(mirrors os/unix/process.rs). Two edits to the -Z build-std sysroot source:

1. library/std/src/sys/process/wasi.rs — after `pub struct ExitStatus(i32);`:
       impl From<i32> for ExitStatus {
           fn from(c: i32) -> ExitStatus { ExitStatus(c) }
       }

2. library/std/src/os/wasi/process.rs — add `FromInner` to the sys import, then:
       #[stable(feature = "rust1", since = "1.0.0")]
       pub trait ExitStatusExt {
           #[stable(feature = "exit_status_from", since = "1.12.0")]
           fn from_raw(raw: i32) -> Self;
       }
       #[stable(feature = "exit_status_from", since = "1.12.0")]
       impl ExitStatusExt for process::ExitStatus {
           fn from_raw(raw: i32) -> Self {
               process::ExitStatus::from_inner(crate::sys::process::ExitStatus::from(raw))
           }
       }

NOTE: -Z build-std caches std by version, not sysroot-src mtime; after editing the
sysroot src, remove target/<target>/*/deps/libstd-* + .fingerprint/std-* to force a rebuild.
