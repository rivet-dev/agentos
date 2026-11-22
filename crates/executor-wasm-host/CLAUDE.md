# Engine-neutral WebAssembly host imports

This crate implements every agentOS WASM host import (Preview1 plus the
`host_*` POSIX extensions) once, for both the V8 and Wasmtime executors.

- Handlers are generic over `WasmGuest`; an engine supplies guest-memory
  access, re-entrant calls into guest exports, and exec preflight compilation.
  Never import an engine crate here.
- Never retain guest-memory references across an await; copy and validate
  input first, then reacquire and revalidate before writing output.
- Linux/POSIX semantics belong in the kernel. Handlers translate the ABI into
  host operations and adapter calls; they own no descriptor, process, or
  filesystem state.
- Shared ABI/profile/error types belong in `agentos-executor-wasm-abi`.
