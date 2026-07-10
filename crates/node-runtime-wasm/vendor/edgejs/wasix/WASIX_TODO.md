# WASIX TODO

This file tracks temporary compatibility workarounds used to get `ubi` building for WASIX.
Items here should be replaced with real WASIX implementations, proper feature gates, or upstream fixes.

## ICU

- Replace the temporary `tzname` / `timezone` / `daylight` UTC stub in [wasix/src/wasix_compat.h](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.h) with a proper WASIX timezone integration once the runtime/sysroot exposes a supported API.
- Validate that the embedded ICU data path is the right long-term packaging model for WASIX. Alternatives to consider:
  - dedicated `icudata` object generation at build time
  - runtime loading from a colocated data file
  - upstreaming a canonical WASIX ICU packaging flow
- Audit the full vendored ICU source build and trim it to the minimal set of libraries/features actually needed by `ubi` if build time or wasm size becomes a problem.

## libc / sysroot gaps

- Replace the `fork()` stub in [wasix/src/wasix_compat.h](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.h) with either:
  - a proper WASIX process model implementation, or
  - an explicit feature disable path in `ubi` where `fork`-style behavior is unsupported.
- Replace the `if_nametoindex()`, `if_indextoname()`, and `getservbyport_r()` stubs in [wasix/src/wasix_compat.h](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.h) with real WASIX-compatible implementations or upstream fixes in the sysroot/libc.
- Revisit the scheduler and thread-name shims in [wasix/src/wasix_compat.h](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.h) and replace them with real support when available.
- Revisit the `ptsname()` stub in [wasix/src/wasix_compat.h](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.h) once PTY support is clearer on WASIX.

## UBI feature stubs

- Revisit the temporary zero-return behavior for process memory APIs in [wasix/src/wasix_compat.cc](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.cc) and callsites in [src/edge_process.cc](/home/theduke/dev/github.com/wasmerio/ubi/src/edge_process.cc). These currently avoid unresolved imports for `uv_get_available_memory`, `uv_get_constrained_memory`, and `uv_resident_set_memory`, but should either report real values or be explicitly feature-gated.
- Fix libuv stdio integration for WASIX instead of relying on compatibility behavior in `ubi` or the harness. Current evidence:
  - guest-side direct WASIX writes work
  - the deeper guest/host memory issue was in the N-API/V8 buffer bridge rather than Wasix stdout capture alone
  - stdio should continue to work through the normal Node/libuv path now that Buffer and typed-array sharing are correct
  The remaining cleanup is to make sure `uv_tty_*`, stdio handle classification, and any TTY-specific behavior work correctly on WASIX without any special-case relabeling or forced handle types.

- Revisit external `ArrayBuffer` / `Buffer` backing semantics in [napi/v8/src/js_native_api_v8.cc](/home/theduke/dev/github.com/wasmerio/ubi/napi/v8/src/js_native_api_v8.cc). WASIX now needs truly shared external backing stores so guest native code and host-side JS see the same bytes. The current fix moved the bridge onto real external backing stores, but this path should be reviewed carefully against upstream `napi-v8` expectations and GC/finalizer behavior.

## Build system

- Stop `ubi/wasix/setup-wasix-deps.sh` from mutating cloned deps during normal builds; switch to pinned revisions or explicit update steps.
- Reduce noisy WASIX compatibility warnings in the build once the functional gaps are resolved.
- Revisit builtin JS loading for WASIX. The current setup is intentionally hybrid:
  - builtin IDs are generated at build time into a compiled catalog, similar to native Node
  - builtin source files are still loaded from host-backed directories mounted into the guest at `/node-lib` and `/node/deps`
  This is good enough for bootstrap, but should be cleaned up into one clear packaging model. Decide whether the long-term direction is:
  - fully guest-visible packaged sources with a stable manifest,
  - host-provided sources with a tighter runner contract, or
  - a different native-aligned mechanism entirely.
  Any cleanup should preserve the requirement that builtin source loads flow through the WASIX filesystem contract rather than ad hoc host-side shortcuts.

## N-API harness

- Revisit getter/setter callback dispatch in [napi/src/napi_bridge_init.cc](/home/theduke/dev/github.com/wasmerio/ubi/napi/src/napi_bridge_init.cc). It currently distinguishes property getters from setters by callback arity when a single N-API property descriptor carries both, which is sufficient for now but should become an explicit callback-kind bridge.
- Replace the temporary no-op `napi_add_env_cleanup_hook()` / `napi_remove_env_cleanup_hook()` bridge in [napi/src/lib.rs](/home/theduke/dev/github.com/wasmerio/ubi/napi/src/lib.rs) with a real cleanup-hook registry that invokes guest callbacks during env teardown.
- Revisit the explicit `ubi_guest_malloc` export used by the WASIX harness for guest-backed `ArrayBuffer` / typed-array creation (see [wasix/src/wasix_compat.cc](/home/theduke/dev/github.com/wasmerio/ubi/wasix/src/wasix_compat.cc) and [napi/src/ctx.rs](/home/theduke/dev/github.com/wasmerio/ubi/napi/src/ctx.rs)). It unblocks correct N-API behavior today, but the long-term interface should likely become a cleaner, generic guest allocator contract rather than a `ubi`-specific exported symbol.
- Remove long-lived raw store-pointer fallback from the callback trampoline in [napi/src/guest/callback.rs](/home/theduke/dev/github.com/wasmerio/ubi/napi/src/guest/callback.rs). Current top-level callback state persists an erased store pointer to support callbacks that fire outside an active `FunctionEnvMut` stack frame (for example interrupt callbacks from [napi/src/napi_bridge_init.cc](/home/theduke/dev/github.com/wasmerio/ubi/napi/src/napi_bridge_init.cc)). Replace this with a queued/deferred callback dispatch model that stores only lightweight callback metadata and drains when a live env/store is available.
