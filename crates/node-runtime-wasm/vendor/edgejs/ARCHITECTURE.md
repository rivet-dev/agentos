# Edge.js Architecture

`edge` is a runtime project that aims to replace Node.js while keeping N-API as
the core boundary. Unlike Node internals that integrate directly with V8 in many
paths, `edge` system bindings should be implemented through `napi/v8` APIs.

## Mission

- Build a Node-compatible runtime architecture centered on N-API contracts.
- Keep engine-specific details isolated behind `napi/v8`.
- Implement system/runtime bindings as N-API modules instead of direct V8 code.
- Advance in small, test-validated milestones.

## Porting Policy

- `edge` source and tests should be ported from Node as fully as possible.
- Preserve upstream structure and behavior semantics by default.
- Only exception: any source path using direct V8 APIs should be adapted to use
  N-API APIs instead.
- Prefer compatibility shims and adapter layers over rewriting upstream logic.
- Hard boundary: files under `src` must never include V8 headers
  (`v8.h`, `libplatform/libplatform.h`) or use `v8::` symbols.
- Host/bootstrap code that requires V8 must live outside `src` (for
  example, under `napi/v8`), while `src` remains N-API/Node-API only.

## Non-Goals (for early phases)

- Full Node parity in one step.
- Immediate support for every Node CLI/runtime flag.
- Rewriting all of Node internals at once.

## Core Architecture Direction

- **Runtime kernel**: process/bootstrap/module-loader/event-loop orchestration.
- **Binding layer**: system features exposed as N-API addons (backed by libuv,
  filesystem/network/process primitives).
- **Engine adapter**: `napi/v8` as the only JS engine integration surface.
- **Compatibility layer**: incremental behavior alignment with Node semantics.

## Build And Binding Organization

The root build is organized as a small target graph rather than one monolithic
runtime source list:

```text
edge_runtime
  edge_runtime_core
  edge_loader
  edge_binding_registry
  edge_bindings
  edge_crypto
  llhttp sources
```

`edge_runtime` remains the stable aggregate target for tests and embedders. The
object targets make ownership explicit while preserving existing public target
names.

Build responsibilities live in focused CMake helpers:

- `cmake/EdgeOptions.cmake`: cache options and target-environment detection.
- `cmake/EdgeNapiProvider.cmake`: N-API provider normalization.
- `cmake/EdgeVendoredDeps.cmake`: vendored runtime dependencies.
- `cmake/EdgeOpenSSL.cmake`: OpenSSL target selection.
- `cmake/EdgeICU.cmake`: ICU target setup.
- `cmake/EdgeBuiltinCatalog.cmake`: generated builtin catalog rules.
- `src/CMakeLists.txt`: runtime source groups.
- `src/binding_registry/CMakeLists.txt`: registry source ownership.
- `src/bindings/CMakeLists.txt`: internal and native binding source ownership.
- `src/crypto/CMakeLists.txt`: crypto source ownership.

Internal bindings are resolved through `src/binding_registry/`. The registry
owns the JS-visible binding manifest, lazy per-environment cache, and teardown
tombstone behavior. `edge_module_loader.cc` only parses the
`internalBinding(name)` argument and asks the registry for the binding. Loader
specific builders such as `builtins`, `contextify`, `modules`, `options`,
`trace_events`, and `uv` are exposed through `src/edge_loader_bindings.h` so
they can remain loader-owned without reintroducing JS-name dispatch in the
loader.

### Adding A Binding

1. Add or extend an initializer with this shape:

   ```cpp
   napi_value InitFoo(napi_env env);
   ```

2. For a substantive internal binding, declare the initializer in
   `src/internal_binding/binding_initializers.h`. For an existing simple native
   installer, use the direct `EdgeInstall*Binding` function.
3. Add one sorted manifest entry in `src/binding_registry/binding_list.h`.
4. Add the source file to `src/bindings/CMakeLists.txt` or the appropriate
   subsystem CMake file.
5. If the binding needs another binding, call
   `edge::binding_registry::Get(env, "dependency")` directly.
6. Update or add registry coverage in `tests/runners/`, then run the dispatch
   guard and the relevant runtime smoke tests.

Private helper exports, such as `os_constants`, should stay out of the
JS-visible manifest unless they are intentionally exposed through
`internalBinding(name)`.

## Roadmap Summary

Detailed milestones are tracked in the public roadmap issue:
<https://github.com/wasmerio/edgejs/issues/8>.

1. **Bootstrap**
   - `edge` executable that creates an environment through `napi/v8`.
   - Run/evaluate JS entry scripts.
2. **Minimal runtime primitives**
   - Implement foundational bindings (`process`, timers, console, basic module
     loading) through N-API.
3. **System binding expansion**
   - Add filesystem/path/os/crypto-like primitives as N-API-based modules.
4. **Node-compat iteration**
   - Port behavior test-by-test; close gaps in semantics and errors.
5. **Hardening and scale**
   - Stability, lifecycle, worker/thread integration, performance regression
     tracking.

## Testing Philosophy

Every roadmap step requires:

- Unit tests for new runtime/binding logic.
- Integration tests for end-to-end behavior from JS entrypoint.
- Compatibility tests aligned with Node expectations where feasible.
- A green gate before moving to the next milestone.

No phase should be marked complete without passing its defined test gate.

## WASIX Build

- Use `EDGE_NAPI_PROVIDER=imports` to compile `edge` with N-API imports only
  (no bundled `napi/v8` linkage).
- WASIX toolchain file: `wasix/wasix-toolchain.cmake`.
- Setup + build helper:
  - `wasix/setup-wasix-deps.sh`
  - `wasix/build-wasix.sh`
