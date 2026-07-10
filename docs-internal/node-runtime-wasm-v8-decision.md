# Node V8 to rusty_v8 decision

Status: **reviewed-delta path selected; R0 acceptance remains gated**

AgentOS keeps the existing `v8` crate and native V8 isolate. It does not embed
Node's V8 build and does not add another JavaScript or WebAssembly engine. The
machine-readable comparison is
[`node-runtime-wasm-v8-delta.json`](./node-runtime-wasm-v8-delta.json), generated
by `scripts/generate-node-runtime-wasm-v8-delta.mjs`.

## Version decision

| Input | V8 | ICU | Identity |
|---|---|---|---|
| Node 24.15.0 | 13.6.233.17-node.48 | 78.2 | Node commit `848430679556aed0bd073f2bc263331ad84fa119` |
| rusty_v8 | 13.6.233.2 | 74.2 | crate 136.0.0, checksum `278d906d3513fce0be40e1b28eb4c482f44e9d3bf7c1be880441e706bebf5e43` |

There is no published rusty_v8 136 patch crate aligned to Node's `.17` build.
R0 therefore takes the reviewed-delta branch allowed by the replacement spec.
The native V8 and ICU versions remain part of the immutable Node runtime pin.

The public-header comparison has 110 exact files, seven modified files, four
Node-only documentation/owner files, and no rusty_v8-only files. The executable
API changes between `.2` and `.17` are limited to isolate deinitialization/free,
explicit-resource-management symbols, an import-assertion deprecation marker,
and platform/compiler guards. AgentOS currently calls none of the added APIs;
the exact Rust `v8::` usage inventory is generated into the delta JSON.

## Behavior gates

The version decision is accepted only when all rows below pass. A source-level
delta does not substitute for behavior evidence.

| Gate | Current result | Evidence or required action |
|---|---|---|
| WebAssembly shared memory, atomics, SIMD, and exception tags | pass | `AGENTOS_V8_SESSION_CASE=v8-wasm-features` production-session test |
| same-isolate WASM export → JS import → WASM reentry, exceptions, traps, and bounded grow | pass | `AGENTOS_V8_SESSION_CASE=nested-node-runtime-probe` |
| snapshots and cached data never cross V8 revisions | pass by construction | snapshots are generated in a helper built from the same runtime binary; no release snapshot or cached module is accepted across identities |
| concurrent snapshot restore while other isolates are live | pass | V8 isolate groups require one read-only snapshot checksum at a time; same-variant isolates restore concurrently, while a mixed live variant deterministically uses a fresh isolate and evaluates its bridge. The full embedded-session suite and `shared-runtime-quota` case pass. |
| ICU 74.2 versus Node ICU 78.2 semantics and data | open | run every required Intl, encoding, locale, timezone, regex-Unicode, and URL identity from the frozen compatibility manifest; any mismatch requires an aligned ICU build or an explicit reviewed behavior row |
| Node engine-extension families | open | every `agentos_node_engine_v1` row must cite its pinned Node caller, rusty_v8 API, lifecycle states, and passing test |
| snapshot format, code cache, and compiled-module reuse | open | version-tag and hostile-corruption tests are mandatory; compiled-module reuse stays disabled unless a supported rusty_v8 API is proven |

R0 cannot pass while a row is failed or open. In particular, the passing WASM
feature probe proves that V8 is the correct engine; it does not waive the
snapshot-concurrency or ICU compatibility work.
