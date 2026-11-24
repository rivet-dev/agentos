# V8 WebAssembly executor

This crate runs standalone WebAssembly commands on V8, the default WASM engine.
Each guest gets its own isolate, and every host import is a Rust callback that
runs the engine-neutral handlers in `agentos-executor-wasm-host`, the same ones
the Wasmtime executor binds. No JavaScript runs in the guest path.

- Fix host-import behavior in `agentos-executor-wasm-host`, never here; this
  crate only binds the handlers to V8 (`native/guest.rs`).
- Shared ABI definitions, validation profiles, permission tiers, and stable
  errors belong in `agentos-executor-wasm-abi`.
- Process-global V8 platform and isolate mechanics belong in
  `agentos-executor-v8-runtime`.
- An isolate runs exactly one guest and is disposed with it. The warm pool
  (`native/pool.rs`) only pre-creates isolates; never reuse one across guests.
- A host import that ends the image records why in `GuestState::unwind` and
  terminates execution; guest code cannot catch termination.
- Never keep a guest-memory pointer across a call into the guest; memory growth
  replaces the backing buffer.
- Behavior shared with Wasmtime must have cross-engine conformance coverage
  (`crates/vm/tests/wasm_raw_abi.rs`, `wasm_software_parity.rs`).
