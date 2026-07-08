# tokio spawn_blocking inline on wasi
wasm32-wasip1 has no OS threads, so tokio's blocking thread pool never spawns
workers — queued spawn_blocking tasks hang forever. codex (sqlx/sqlite, rollout
writes, etc.) calls spawn_blocking during a turn and HUNG (no panic). Fix
(runtime/blocking/pool.rs spawn_blocking): on #[cfg(target_os="wasi")] run the work
as an ordinary current-thread task — `crate::task::spawn(async move { func() })` —
instead of the pool. codex's blocking ops are short + cooperative on the single-threaded VM.
(Complements 0002 fs asyncify, which runs fs ops inline.)

## UPDATE: run inline synchronously (not deferred) to avoid single-thread deadlock
The first version `crate::task::spawn(async move { func() })` defers func() to a task,
which can DEADLOCK on the single thread (a deferred blocking task waits on a task that
can't run because the thread is busy). Corrected: run `let result = func();` INLINE
(synchronously, completing before any other task interleaves), then return a ready
JoinHandle via `crate::task::spawn(async move { result })`. Matches the fs-asyncify
inline approach. spawn_blocking work is synchronous (FnOnce()->R) so inline is safe.
