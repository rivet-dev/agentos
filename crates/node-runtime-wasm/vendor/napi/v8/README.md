 # napi/v8

 `napi/v8` is a standalone compatibility layer that implements Node-API (N-API)
 on top of V8 while keeping V8 details internal.

 ## Scope

 - Public surface: N-API headers and ABI (`js_native_api.h`, `node_api.h`).
 - Internal implementation: V8-backed code only in `src/`.
 - Host/runtime services such as `libuv` loops and memory queries are owned by
   Edge native code and injected through unofficial embedder hooks.
 - Test strategy: keep standalone `napi/v8` coverage focused on portable V8
   adapter behavior first.

 ## Porting Policy

 - Source and tests should be ported from upstream Node as fully as possible.
 - Keep upstream files/logic verbatim unless adaptation is strictly required.
 - The only intended code adaptation rule: replace direct V8 API usage with
   equivalent N-API usage.
 - Favor harness/environment shims over rewriting upstream test content.

 ## Layout

- `../include/`: shared public C headers (engine-agnostic surface)
- `../tests/`: shared canonical Node-API fixtures
- `src/`: V8-backed implementation and environment glue, with no direct
  `libuv` ownership
- `tests/`: V8-specific test assets and compatibility test docs

 ## Current Phase

 This directory implements the standalone V8 adapter plus a portable test
 subset. Runtime-owned Node-API behavior that depends on host event-loop or
 `libuv` services is validated from the Edge runtime build rather than this
 package.

See `tests/README.md` for build/run instructions and
`tests/PORTABILITY_MATRIX.md` for the current portability classification.
