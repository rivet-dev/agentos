# tokio fs asyncify inline on wasi
tokio::fs::* uses asyncify() → spawn_blocking, which panics on single-threaded
wasm32-wasip1 (no blocking thread pool): "RuntimeError: unreachable" at fs/mod.rs.
codex reads config via tokio::fs and hits this after emitting {"type":"start"}.
Fix (src/fs/mod.rs asyncify): on #[cfg(target_os="wasi")] run the closure inline
(`return f();`) instead of spawn_blocking. NOTE: sqlx/other spawn_blocking callers may
need a broader fix (make spawn_blocking itself run inline on wasi) if they panic at runtime.
