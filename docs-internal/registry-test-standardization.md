# Software test layout

Status: superseded by `docs-internal/registry-flatten-colocation-spec.md`.

The old registry-wide test plan assumed centralized package tests and a native
build tree under the removed wrapper directory. That layout has been replaced:

- package e2e tests live under `software/<pkg>/test/`;
- shared VM test helpers live in the private `@rivet-dev/agentos-test-harness` workspace
  package;
- libc/sysroot conformance tests live under `toolchain/conformance/`;
- C VM test fixtures live under `toolchain/test-programs/`;
- native command source is colocated under `software/<pkg>/native/`, while
  shared build infrastructure remains under `toolchain/`.

Use `scripts/check-layout.mjs` for the current structural gate.
