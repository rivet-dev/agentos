# Wasmtime WebAssembly executor

This crate owns Wasmtime engine configuration, Stores, linker adaptation,
checked guest-memory access, compilation caching, interruption, and optional
thread-group mechanics.

The host-import handlers themselves live in `agentos-executor-wasm-host` and
are shared with the V8 executor. This crate binds them through `WasmGuest`;
fix import behavior there, not here.

- Do not use `wasmtime-wasi` or ambient host capabilities. Link the agentOS
  Preview1 plus POSIX ABI to bounded executor-contract capabilities.
- Never retain guest-memory references across an async wait; copy and validate
  input first, then reacquire and revalidate before writing output.
- Shared ABI/profile/error types belong in `agentos-executor-wasm-abi`.
- Linux/POSIX semantics belong in the kernel, not in Wasmtime-specific code.
- Serialized/AOT artifacts remain out of scope unless separately designed.
