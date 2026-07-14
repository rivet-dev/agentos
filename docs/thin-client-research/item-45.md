# Item 45 research — remove JSON frame and legacy fixture codecs

Status: implementation-ready research only. This note changes no implementation,
test, generated protocol, tracker status, or revision.

## Decision

| Field | Finding |
| --- | --- |
| Original issue | Production Rust and TypeScript protocol packages still expose JSON-frame compatibility and codec-selection APIs, while Rust production sidecars accept only the generated BARE wire. Rust production code also contains a large string-map parser used only to support old tests. |
| Priority | **P2** — this is unnecessary public/client complexity and misleading compatibility behavior, not a current production escape. |
| Confidence | **High** — repository-wide references identify all consumers, real native/browser sidecars are BARE-only, and JSON/legacy-config consumers are compatibility tests or options used by test fakes. |
| Recommended fix | Delete both Rust and TypeScript JSON frame codecs and every codec-selection option; keep one BARE transport. Replace Rust string-map fixtures with typed `CreateVmConfig` and replace JSON fake sidecars with generated BARE fixtures. Do not move any compatibility codec into the sidecar. |

Tracker anchors: `docs/thin-client-migration.md:91`, status row at line 171,
and Item 45 checklist at line 256.

## Original issue and current evidence

The previous Item 45 note covered only the Rust half. The complete inventory is:

1. `crates/sidecar-protocol/src/protocol.rs:1883-2054` defines
   `NativePayloadCodec::{Json,Bare}` and `NativeFrameCodec`. Its decoder sniffs
   the first byte and tries an alternate decoder. The only consumers are
   `crates/native-sidecar/tests/protocol.rs` and
   `crates/native-sidecar/tests/generated_protocol.rs`.
2. `crates/sidecar-protocol/src/wire.rs:134-157` defines
   `CreateVmRequest::legacy_test_config`. It has **23 direct call sites across
   13 native-sidecar test files** and no production caller. Shared and local
   metadata helpers expand that to dozens of fixture invocations. It delegates
   to a roughly 500-line parser of string keys such as
   `resource.max_processes` and `env.NAME`.
3. `packages/runtime-core/src/frame-payload-codec.ts:1-25` is a second JSON
   codec. `packages/runtime-core/src/protocol-frames.ts:91,200-217` makes JSON
   versus BARE a production/public choice.
4. That TypeScript choice propagates through native stdio and synchronous browser
   APIs as `payloadCodec?` or `codec?`. It is also published as the
   `@rivet-dev/agentos-runtime-core/frame-payload-codec` subpath.
5. The real sidecars do not negotiate or decode JSON. Native stdio constructs
   `WireFrameCodec` at `crates/native-sidecar/src/stdio.rs:161`; the browser
   sidecar constructs it at
   `crates/native-sidecar-browser/src/wire_dispatch.rs:89,122`. Therefore the
   TypeScript JSON choice only works with JSON test doubles and is invalid
   against the real same-version sidecar.

### Current-tree caller inventory

The following inventory was reverified on the Item 80 working tree. It is the
handoff list for implementation; do not rediscover or retain any of these
adapters for convenience.

| Compatibility surface | Production definition/plumbing | Actual callers |
| --- | --- | --- |
| Rust handwritten JSON/BARE frame codec | `crates/sidecar-protocol/src/protocol.rs:1883-2054` | Only `crates/native-sidecar/tests/protocol.rs` and `generated_protocol.rs`; native stdio and browser production both use `wire::WireFrameCodec`. |
| Rust string-map create config | `crates/sidecar-protocol/src/wire.rs:134-638` | 23 direct calls in `builtin_conformance`, `connection_auth`, `fs_watch_and_streams`, `guest_identity`, `kill_cleanup`, `layer_management`, `permission_flags`, `protocol`, `python`, `service`, `session_isolation`, `stdio_binary`, and `support/mod.rs`. |
| Shared Rust metadata fixture | `crates/native-sidecar/tests/support/mod.rs:227-272,423-504` | 21 external `create_vm_wire_with_metadata` calls in `builtin_completeness`, `fetch_via_undici`, `posix_path_repro`, `promisify_module_load`, `security_hardening`, `signal`, and `socket_state_queries`; the remaining occurrences are wrapper definition/delegation. |
| Local Rust metadata fixtures | `builtin_conformance.rs:151`, `python.rs:510`, `service.rs:1339` | File-local tests only. `service.rs` has 20 calls plus the local helper definition; do not confuse these with similarly named shared helpers. |
| TypeScript JSON payload implementation | `packages/runtime-core/src/frame-payload-codec.ts` plus selectors in `protocol-frames.ts`, `protocol-client.ts`, `native-client.ts`, and `sidecar-process.ts` | All explicit `payloadCodec: "json"` uses are tests: runtime-core protocol/native-client tests and core native-sidecar-process tests. |
| Browser codec selector | runtime-browser/browser `codec?: ProtocolFramePayloadCodec` options and forwarding | All explicit `codec: "json"` uses are fake-sidecar unit tests; all explicit `codec: "bare"` uses are redundant integration/harness options. The actual WASM sidecar accepts BARE only. |

The only string metadata keys still used by those Rust fixtures are:

```text
cwd
env.AGENTOS_ALLOWED_NODE_BUILTINS
env.AGENTOS_KEEP_STDIN_OPEN
env.AGENTOS_LOOPBACK_EXEMPT_PORTS
env.VISIBLE_MARKER
env.WORKTREE
limits.http.max_fetch_response_bytes
network.dns.override.example.test
network.dns.override.metadata.test
network.dns.servers
resource.cpu_count
resource.max_pread_bytes
resource.max_processes
resource.max_sockets
resource.max_wasm_fuel
resource.max_wasm_memory_bytes
```

Everything else recognized by `legacy_dns_config`,
`legacy_native_root_config`, `legacy_listen_config`, and
`legacy_limits_config` is already dead fixture grammar. The migration must not
turn those unused keys into a new helper API.

Before implementation, capture the inventory with:

```sh
rg -n 'NativeFrameCodec|NativePayloadCodec|legacy_test_config' crates
rg -n -e payloadCodec -e ProtocolFramePayloadCodec -e TransportPayloadCodec \
  -e encodeJsonFramePayload -e decodeJsonFramePayload -e frame-payload-codec \
  packages --glob '!**/dist/**' --glob '!**/generated/**'
rg -n -e 'codec: "bare"' -e 'codec: "json"' -e 'payloadCodec:' \
  packages --glob '!**/dist/**' --glob '!**/generated/**'
```

The current tests that explicitly preserve the behavior are the required
before-behavior evidence, not behavior to retain:

- `packages/runtime-core/tests/frame-payload-codec.test.ts:10-44` proves JSON
  frame encoding and the special `process_output.chunk` revival.
- `packages/runtime-core/tests/protocol-frames.test.ts:270-299` proves a JSON
  compatibility frame roundtrip.
- `packages/runtime-core/tests/protocol-client.test.ts:21-103` makes JSON the
  stdio test default, and lines 142, 179, 217, 295, 344, and 373 pass the option
  even to injected typed transports where it has no effect.
- `packages/runtime-core/tests/native-client.test.ts:14-133` launches two JSON
  fake sidecars and selects them with `payloadCodec: "json"`.
- `crates/native-sidecar/tests/protocol.rs:258-291,401-420` proves JSON
  roundtrip and JSON/BARE autodetection.
- `crates/sidecar-protocol/src/wire.rs:1204-1240` has three
  `legacy_metadata_preserves_*` parser tests.
- The browser/core fake-sidecar tests listed below explicitly select JSON and
  therefore prevent deleting the production option today.

Do not infer a compatibility promise from these tests. The repository contract
says the protocol, clients, and sidecars release in lockstep with no wire
backward-compatibility guarantee.

## Root cause

This is unfinished migration scaffolding, not a sidecar capability. The real
native and browser entrypoints moved to generated `WireFrameCodec`/BARE, but the
older handwritten Rust codec was left publicly exported for compatibility
tests. TypeScript then retained a matching public codec selector so JSON fake
sidecars could keep working. Separately, native tests kept constructing VM
configuration through the pre-typed string metadata map, which caused its
roughly 500-line parser to remain compiled into the production protocol crate.

Nothing on a real same-version connection negotiates these choices. Moving the
old codecs or metadata parser into the sidecar would therefore add behavior;
the correct fix is to migrate the fixtures and delete the compatibility paths.

## What must remain client-side

This item removes alternate behavior, not transport itself. Keep these client
responsibilities:

- four-byte big-endian length framing and generated BARE encode/decode;
- conversion between ergonomic TypeScript request/response objects and the
  generated positional BARE types;
- validation and serialization of explicit caller input;
- routing host callbacks/events and retaining host-only correlation state;
- TypeScript Zod tool-schema construction and the package-manager default
  package exception, neither of which is related to this item.

Keep `encodeBareProtocolFrame`, `decodeBareProtocolFrame`, and
`toGeneratedProtocolFrame` in
`packages/runtime-core/src/protocol-frames.ts:155-198`. Keep Rust
`wire::WireFrameCodec` at `crates/sidecar-protocol/src/wire.rs:815-925`.

Also keep JSON text that is an explicitly typed value *inside* a BARE frame.
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:5,167-170` defines
`JsonUtf8` and `CreateVmRequest.config: JsonUtf8`.
`CreateVmRequest::json_config` at `wire.rs:124-132` serializes typed
`CreateVmConfig` into that field. ACP JSON-RPC, tool schemas/results, extension
payloads, and other `JsonUtf8` values remain. Consequently `serde_json` remains
a real dependency. A nested JSON string does not justify a JSON frame codec.

## Exact Rust production edits

### Delete the duplicate frame codec

In `crates/sidecar-protocol/src/protocol.rs`, delete the complete compatibility
block at current lines 1883-2054:

- `serialize_payload` and `deserialize_payload`;
- `NativePayloadCodec`, including `sniff` and `alternate`;
- `NativeFrameCodec`, codec overrides, fallback decoding, and `Default`.

Do not add a BARE-only wrapper with the old names. Keep
`to_generated_protocol_frame` and `from_generated_protocol_frame`; native
internals still cross the handwritten/generated model boundary explicitly.
Deleting the entire handwritten compatibility model is a separate migration.

### Delete the stringly test-config parser

In `crates/sidecar-protocol/src/wire.rs`:

- keep `CreateVmRequest::json_config` at lines 124-132;
- delete `legacy_test_config` at lines 134-157;
- delete its private-only parsing helpers: `legacy_env_config` (160), root
  converters (172, 202, 221), legacy DNS (341), native root (374), listen
  policy (394), loopback exemptions (419), limits (435), `legacy_u64` (568),
  and `legacy_has_*` (576-638);
- keep `permissions_policy_config_from_wire` at lines 254-273. Native configure
  uses it at `crates/native-sidecar/src/vm.rs:464`, browser configure uses it at
  `crates/native-sidecar-browser/src/wire_dispatch.rs:749`, and service tests
  also exercise it;
- rename its surviving private helpers to remove the false “legacy” label:
  `legacy_permission_mode_config` -> `permission_mode_config_from_wire`,
  `legacy_fs_permission_scope_config` ->
  `fs_permission_scope_config_from_wire`, and
  `legacy_pattern_permission_scope_config` ->
  `pattern_permission_scope_config_from_wire`;
- delete only the three `legacy_metadata_preserves_*` unit tests at current
  lines 1204-1240. Retain permission defaults, mount defaults, and package-source
  BARE tests.
- update the `WireFrameCodec::encode_message` documentation at current lines
  858-865: it currently names the soon-to-be-deleted
  `encodeProtocolFramePayload(frame, "bare")`; refer directly to
  `encodeBareProtocolFrame` and the raw message boundary instead.

No Cargo dependency or lockfile deletion follows from this block.

### Replace stale protocol documentation

Rewrite `crates/sidecar-protocol/protocol/README.md` from a migration plan to
the current BARE contract. Preserve framing, ownership, request-ID, bounds, and
`JsonUtf8` explanations, but explicitly state:

> The four-byte big-endian length prefix contains exactly one BARE
> `ProtocolFrame`. There is no JSON frame codec, codec negotiation, first-frame
> sniffing, or fallback decoder. `JsonUtf8` fields are JSON text nested inside
> the BARE frame.

Delete the dual-stack rollout and JSON-frame normalization steps.

Update `crates/CLAUDE.md:100` to say the wire payload is always generated BARE,
while dynamic `JsonUtf8` values remain nested typed fields. The current
“payload-codec changes” wording is migration-era guidance.

## Exact TypeScript production edits

### Runtime core: one BARE path

| File and anchor | Exact edit |
| --- | --- |
| `packages/runtime-core/src/frame-payload-codec.ts:1-25` | Delete the file: `TransportPayloadCodec`, `encodeJsonFramePayload`, and `decodeJsonFramePayload` have no supported use after fixture migration. |
| `packages/runtime-core/src/protocol-frames.ts:2-6,91,200-217` | Remove the JSON imports/type alias and delete `encodeProtocolFramePayload` / `decodeProtocolFramePayload`. Callers use the explicit one-argument `encodeBareProtocolFrame` / `decodeBareProtocolFrame` at lines 188-198. Do not retain a meaningless one-value codec type. |
| `packages/runtime-core/src/protocol-client.ts:26,44-52,74,99-119` | Remove `payloadCodec` from `SidecarProtocolClientOptions`, remove the stored field/default, and wire `FrameRpcTransport` directly to the BARE functions. |
| `packages/runtime-core/src/native-client.ts:9-14,23-50,69-74,103-123` | Remove `payloadCodec` from `StdioSidecarProtocolClientSpawnOptions`, its resolved `Pick`, forwarding, and default. |
| `packages/runtime-core/src/sidecar-process.ts:31,188,192-216,392-409` | Delete `NativeTransportPayloadCodec`, `SidecarSpawnOptions.payloadCodec`, the resolved field, and spawn forwarding/default. |
| `packages/runtime-core/src/index.ts:7` | Remove the deleted file’s root re-export. Keep protocol/frame exports. |
| `packages/runtime-core/package.json:144-148` | Remove the public `./frame-payload-codec` export. |
| `packages/runtime-core/README.md:3-7` | Describe BARE-only transport primitives and remove the deleted subpath from the export list. |

This deliberately removes a public compatibility option. No deprecation shim
is appropriate because same-version lockstep is the stated protocol model.

### Browser transport: remove codec plumbing

| File and anchor | Exact edit |
| --- | --- |
| `packages/runtime-browser/src/converged-sync-bridge-handler.ts:17-21,55-60,69-98` | Remove the codec import, field, constructor option/default, and stale JSON limitation comment. `PushFrameSidecarTransport.sendRequest` uses `encodeBareProtocolFrame` and `decodeBareProtocolFrame` directly. |
| `packages/runtime-browser/src/converged-executor-session.ts:15,36-39,55-63,164-185` | Remove `ConvergedExecutorSessionOptions.codec`, the stored default, and both forwarding sites. |
| `packages/runtime-browser/src/converged-driver-setup.ts:12,27-37,69-75` | Remove `ConvergedServicerOptions.codec` and forwarding. |
| `packages/runtime-browser/src/runtime-driver.ts:1,86-95,540-553` | Remove `ConvergedSidecarFactoryOptions.codec` and forwarding into the servicer. |
| `packages/runtime-browser/src/default-sidecar.ts:15,38-50,57-66` | Remove `DefaultConvergedSidecarOptions.codec` and the returned factory field. The real WASM sidecar is already BARE-only. |
| `packages/browser/src/converged-sidecar.ts:19,72-76,118-133` | Remove `AgentOsConvergedSidecarOptions.codec` and the returned factory field. |

Update `packages/core/CLAUDE.md:36`: replace “defaults to BARE; keep JSON behind
migration options” with “the framed sidecar wire is BARE-only; never add a JSON
codec selector.” The root `CLAUDE.md:65-75` already clearly states the thin-client
rule and needs no Item 45 change.

## Exact Rust fixture migration

### Shared support API

At `crates/native-sidecar/tests/support/mod.rs:227-272,423-504`, replace
`create_vm_wire_with_metadata` and `create_vm_with_metadata` with typed helpers:

```rust
pub fn create_vm_wire_with_config(
    sidecar: &mut NativeSidecar<RecordingBridge>,
    request_id: RequestId,
    connection_id: &str,
    session_id: &str,
    runtime: wire::GuestRuntimeKind,
    host_workspace: &Path,
    config: agentos_vm_config::CreateVmConfig,
) -> (String, wire::WireDispatchResult)
```

The request-building part of the helper only calls
`CreateVmRequest::json_config(runtime, config)` and dispatches. After VM
creation it must retain Item 80's explicit test-operator mount by calling
`configure_host_workspace_mount(..., host_workspace)`. `host_workspace` is
therefore test harness state and **must never be serialized as `cwd`**. The
helper must not parse metadata, add environment variables, translate root
descriptors, or fill runtime defaults.

Keep the simple `create_vm_wire` and `create_vm` helpers. They should pass
`CreateVmConfig::default()`: leave `cwd`, `root_filesystem`, and `permissions`
omitted so the sidecar owns all three defaults, including its documented
allow-all permission default. Empty-map callers use those simple helpers.
Tests that intentionally exercise an explicit allow-all policy can use
`agentos_native_sidecar_core::allow_all_policy()` (or a small test-only typed
wrapper) instead of converting `wire_permissions_allow_all()`. Root-filesystem
and permission tests construct `agentos_vm_config` types directly.

Map non-empty fixtures directly:

| Old metadata | Typed fixture field |
| --- | --- |
| `cwd` | `CreateVmConfig.cwd` |
| `env.NAME` | `CreateVmConfig.env` |
| `env.AGENTOS_ALLOWED_NODE_BUILTINS` | Parse the fixture JSON once into `Vec<String>` and set `CreateVmConfig.js_runtime.allowed_builtins`; do not preserve the internal env knob. |
| `env.AGENTOS_LOOPBACK_EXEMPT_PORTS` | `CreateVmConfig.loopback_exempt_ports` |
| `resource.*` | `VmLimitsConfig.resources` |
| `limits.http.*` | `VmLimitsConfig.http` |
| `limits.js_runtime.*`, `limits.python.*`, `limits.wasm.*` | matching typed nested limit config |
| `network.dns.*` | `CreateVmConfig.dns` |
| `network.listen.*` | `CreateVmConfig.listen` |
| wire root descriptor | `RootFilesystemConfig` / typed lower and entry values |
| wire permission policy | typed `PermissionsPolicy`, or retained `permissions_policy_config_from_wire` only at an intentional wire boundary |

`AGENTOS_KEEP_STDIN_OPEN` may remain explicit `CreateVmConfig.env` test input;
there is no canonical create-config field for it today. Invalid permission-rule
tests must construct invalid typed rule sets directly so validation still runs.
Root/layer tests must preserve explicit snapshot/lower behavior rather than
replacing it with `Default::default()`.

The file-by-file conversion is:

| Fixtures | Exact typed replacement |
| --- | --- |
| `builtin_completeness`, `promisify_module_load`, `signal`, `socket_state_queries` | `JsRuntimeConfig { allowed_builtins: Some(...), ..Default::default() }`; empty metadata calls use the simple helper. |
| `fetch_via_undici` | `loopback_exempt_ports: Some(vec![port])`; do not also set `AGENTOS_LOOPBACK_EXEMPT_PORTS` in `env`. |
| `posix_path_repro` | Typed `js_runtime.allowed_builtins`; retain only `WORKTREE` in `env`. |
| `security_hardening` | Retain `VISIBLE_MARKER` in `env`; put `max_processes` in `limits.resources`. |
| `builtin_conformance` | Convert allowed builtins, loopback ports, DNS servers, `cpu_count`, and `max_wasm_memory_bytes` to their canonical typed fields. Keep only `AGENTOS_KEEP_STDIN_OPEN` as explicit env. |
| `python` | Convert loopback ports and DNS overrides to `loopback_exempt_ports` and `dns.overrides`; its root-filesystem helper takes typed `RootFilesystemConfig`. |
| `service` | Convert `max_pread_bytes`, `max_wasm_fuel`, `max_sockets`, `max_fetch_response_bytes`, loopback ports, and DNS overrides to typed fields. Rename its local helper to `create_vm_with_config`. |
| `connection_auth`, `session_isolation`, `kill_cleanup` | Use `CreateVmRequest::json_config` with the smallest typed config; these are ownership/state rejection tests and need no root descriptor. |
| `layer_management`, `guest_identity`, `fs_watch_and_streams`, `stdio_binary` | Convert each explicit root descriptor to `RootFilesystemConfig`; retain the exact lowers/bootstrap entries being asserted. |
| `permission_flags` | Construct `agentos_vm_config::PermissionsPolicy` directly, including invalid empty-operation fixtures, so canonical config validation remains under test. |
| `protocol`, `generated_protocol` | Use generated wire frames and typed config. The old metadata key `runtime=wasm` is ignored by the parser today and must simply disappear. |

Direct `legacy_test_config` callers are in:

- `builtin_conformance.rs`, `connection_auth.rs`,
  `fs_watch_and_streams.rs`, `guest_identity.rs`, `kill_cleanup.rs`,
  `layer_management.rs`, `permission_flags.rs`, `protocol.rs`, `python.rs`,
  `security_hardening.rs`, `service.rs`, `session_isolation.rs`,
  `stdio_binary.rs`, and `support/mod.rs` under
  `crates/native-sidecar/tests/`.

Indirect `*_with_metadata` callers additionally occur in
`builtin_completeness.rs`, `fetch_via_undici.rs`, `posix_path_repro.rs`,
`promisify_module_load.rs`, `signal.rs`, and `socket_state_queries.rs`.
Rename local helpers in `builtin_conformance.rs` and `service.rs` to
`*_with_config`; neither file should retain a generic string map adapter.

## Exact frame and TypeScript test migration

### Rust protocol tests

- In `crates/native-sidecar/tests/protocol.rs:44-445`, migrate frame roundtrips
  to generated `wire::{ProtocolFrame,RequestPayload,...}` and
  `WireFrameCodec`. Delete the JSON roundtrip at 258 and replace autodetection
  at 401 with `wire_codec_rejects_legacy_json_payload`, using a length-prefixed
  static JSON object and expecting `ProtocolCodecError::DeserializeFailure`.
  Use `WireFrameCodec::new(64)` for the bound test. Convert all CreateVM payloads
  to typed `json_config`.
- In `crates/native-sidecar/tests/generated_protocol.rs:53-138`, encode/decode
  with `WireFrameCodec` and call `to_generated_protocol_frame` /
  `from_generated_protocol_frame` explicitly around the compatibility model.
- At `crates/native-sidecar/tests/protocol.rs:905-1010`, keep schema coverage,
  rename `BARE_MIGRATION_PLAN` to `BARE_PROTOCOL_DOC`, assert the BARE-only
  wording, and reject “JSON frames begin”, “dual-stack”, and “first successfully
  decoded frame”.

### TypeScript fixture strategy

Do **not** add inverse generated-to-live request conversion to production merely
to make fake sidecars convenient. Test-side sidecar emulation may decode and
construct generated BARE unions directly.

Recommended reusable test fixtures:

- add `packages/runtime-core/tests/fixtures/bare-sidecar.ts` for the two spawned
  stdio smoke tests. Launch it with `process.execPath` and
  `args: ["--import", "tsx", fixturePath]`; root `package.json` already provides
  the loader. It imports
  `src/generated-protocol`, decodes a generated `RequestFrame`, and writes a
  generated `ResponseFrame` with the existing four-byte prefix;
- add `packages/runtime-browser/tests/support/fake-bare-sidecar.ts` for the
  in-process bootstrap/filesystem/kernel fixture variants used by the runtime
  browser unit tests;
- add `packages/core/tests/fixtures/bare-sidecar.ts` with explicit modes for the
  permissions capture, event overflow, and child-exit fixtures. It must decode
  `CreateVmRequest.config` as the nested `JsonUtf8` string when inspecting
  permissions. Reuse generated BARE functions instead of extending the manual
  partial codec string currently embedded as `BARE_FIXTURE_PROTOCOL_HELPERS` in
  `native-sidecar-process.test.ts:94-279` unless that existing partial helper is
  first generalized into this test-only fixture.

Then make these exact test edits:

| Test file | After edit |
| --- | --- |
| `packages/runtime-core/tests/frame-payload-codec.test.ts` | Delete with the codec. |
| `packages/runtime-core/tests/protocol-frames.test.ts:1-10,270-299` | Delete the JSON roundtrip imports/test; add a focused assertion that `decodeBareProtocolFrame` rejects static JSON bytes. Retain generated-byte and binary BARE coverage. |
| `packages/runtime-core/tests/protocol-client.test.ts:21-103` | Make stdio helpers read/write generated BARE frames. Remove every `payloadCodec` option; injected `MemoryFrameTransport` tests need no byte codec at all. |
| `packages/runtime-core/tests/native-client.test.ts:14-133` | Replace both generated JSON scripts with the BARE fixture and remove the option. Preserve stdio and shared `SidecarProcess.spawn` behavior assertions. |
| `packages/runtime-browser/tests/runtime/converged-sync-bridge-handler.test.ts:184-240` | Replace JSON byte fixtures with the shared BARE fixture; preserve real frame roundtrip and rejected-response behavior. |
| `packages/runtime-browser/tests/runtime/converged-executor-session.test.ts:1-190` | Use the shared BARE fake and remove five codec selections; preserve ownership, bootstrap, package forwarding, execution registration, and pre-bootstrap error assertions. |
| `packages/runtime-browser/tests/runtime/converged-driver-setup.test.ts:1-185` | Use the BARE fake and remove codec selection. The current test catches failures because JSON cannot roundtrip `ArrayBuffer`; after BARE migration assert the binary guest-kernel result normally instead of swallowing that failure. |
| `packages/runtime-browser/tests/runtime-driver/fake-converged-sidecar.ts:1-64` | Replace the JSON fake implementation/comment with the shared generated-BARE fixture and remove `codec` from returned factory options. |
| `packages/runtime-browser/tests/integration/converged-wasm.test.ts:54,108,120` | Remove redundant explicit `codec: "bare"`; preserve real-WASM integration behavior. |
| `packages/runtime-browser/tests/browser/fixtures/frontend/converged-harness.entry.ts:64,221,251,284` | Remove redundant `codec: "bare"` from real browser harness sessions. |
| `packages/browser/tests/runtime-driver/converged-sidecar.test.ts:131-149` | Rename the default test to assert config/loader only; delete the codec override assertion while retaining `onFsReadDenied`. |
| `packages/browser/tests/browser-wasm/async-kernel.worker.ts:146` | Remove redundant `codec: "bare"`. |
| `packages/core/tests/native-sidecar-process.test.ts:560-713` | Use generated BARE fixture modes for overflow and child exit; remove both JSON options. Preserve bounded-buffer and immediate-disconnect assertions. |
| `packages/core/tests/native-sidecar-process-permissions.test.ts:99-181` | Use the BARE capture fixture and remove JSON selection; preserve assertions that omitted config fields stay omitted and permission shapes arrive unchanged. |

## Required before/after checklists

### New before-test that fails on the current parent

Add `production_protocol_has_no_compatibility_codecs` to
`crates/native-sidecar/tests/architecture_guards.rs`. The guard should read the
specific production files and package manifest listed above and reject these
symbols/surfaces:

- Rust: `NativePayloadCodec`, `NativeFrameCodec`, and `legacy_test_config`;
- TypeScript: `TransportPayloadCodec`, `ProtocolFramePayloadCodec`,
  `encodeJsonFramePayload`, `decodeJsonFramePayload`, `payloadCodec`, and the
  public `./frame-payload-codec` export;
- browser production options named `codec` whose type is the removed protocol
  codec selector.

Run it before implementation with:

```sh
cargo test -p agentos-native-sidecar --test architecture_guards \
  production_protocol_has_no_compatibility_codecs
```

It must fail on the current parent and print the exact file/symbol inventory.
After fixture migration and deletion it becomes the permanent regression guard.
Keep literal legacy-JSON rejection bytes confined to tests so the guard does
not prohibit proving that `WireFrameCodec` rejects an old frame.

### Before behavior evidence

- [ ] Record the three repository inventories above, including the TypeScript
  public selector and every explicit BARE/JSON test option.
- [ ] Run and record `cargo test -p agentos-sidecar-protocol` and
  `cargo test -p agentos-native-sidecar --test protocol --test generated_protocol`.
  Identify the JSON roundtrip/autodetection and three legacy metadata tests in
  the result.
- [ ] Run and record the focused TypeScript tests before changes:
  `frame-payload-codec`, `protocol-frames`, `protocol-client`, `native-client`,
  the three converged runtime unit files, browser converged-sidecar, and the two
  core native-sidecar-process files. These prove the compatibility surface is
  test-owned and establish the non-codec behavior to preserve.

### After behavior tests

- [ ] `rg` finds no `NativeFrameCodec`, `NativePayloadCodec`,
  `legacy_test_config`, `*_with_metadata`, `TransportPayloadCodec`,
  `ProtocolFramePayloadCodec`, `encodeJsonFramePayload`,
  `decodeJsonFramePayload`, `frame-payload-codec`, `payloadCodec`, or protocol
  `codec:` option in source/tests/docs (unrelated JSON/module codecs excluded).
- [ ] The new `production_protocol_has_no_compatibility_codecs` architecture
  guard passes and remains part of the normal native-sidecar test target.
- [ ] Rust protocol tests use only `WireFrameCodec`, reject a legacy JSON frame,
  retain generated byte parity, and pass after typed config fixture migration.
- [ ] TypeScript protocol tests use only generated BARE frames, reject static
  JSON bytes, and preserve binary `Uint8Array`/`ArrayBuffer` payloads without
  special JSON revival.
- [ ] Native stdio, browser sync transport, event overflow, child exit,
  permissions capture, callback correlation, ownership, package forwarding,
  and real-WASM integration tests still pass.
- [ ] Protocol and CLAUDE/README text says BARE-only and accurately distinguishes
  nested `JsonUtf8`.
- [ ] Only after all checks pass, mark all three Item 45 tracker checkboxes and
  its status row `done` with the dedicated `jj` revision ID.

## Validation commands

Focused Rust validation:

```sh
cargo test -p agentos-sidecar-protocol
cargo test -p agentos-native-sidecar --test protocol --test generated_protocol
cargo test -p agentos-native-sidecar --tests --no-run
cargo test -p agentos-native-sidecar-browser --test wire_dispatch
cargo fmt --check
cargo check --workspace
```

Run affected native integration targets with `cargo nextest`, which isolates
the shared V8 runtime per test process and avoids the broad libtest segfault:

```sh
cargo nextest run -p agentos-native-sidecar \
  --test builtin_completeness --test builtin_conformance \
  --test connection_auth --test fetch_via_undici \
  --test fs_watch_and_streams --test guest_identity --test kill_cleanup \
  --test layer_management --test permission_flags --test posix_path_repro \
  --test promisify_module_load --test python --test security_hardening \
  --test service --test session_isolation --test signal \
  --test socket_state_queries --test stdio_binary
```

Focused TypeScript validation:

```sh
pnpm --dir packages/runtime-core exec vitest run \
  tests/protocol-frames.test.ts tests/protocol-client.test.ts \
  tests/native-client.test.ts
pnpm --dir packages/runtime-core check-types
pnpm --dir packages/runtime-browser test:unit
pnpm --dir packages/runtime-browser test:integration
pnpm --dir packages/runtime-browser check-types
pnpm --dir packages/browser exec vitest run \
  tests/runtime-driver/converged-sidecar.test.ts
pnpm --dir packages/browser check-types
pnpm --dir packages/core exec vitest run \
  tests/native-sidecar-process.test.ts \
  tests/native-sidecar-process-permissions.test.ts
pnpm --dir packages/core check-types
pnpm check-types
pnpm build
```

Run `pnpm --dir packages/runtime-browser test:browser` and
`pnpm --dir packages/browser test:browser-wasm` in the explicit expensive phase
because browser harness codec options also change.

## Dependencies and risks

- **Item 46 follows this item.** Typed fixtures should preserve omission where
  current types permit it, but Item 45 must not opportunistically redesign
  presence semantics. The typed migration makes Item 46’s missing-presence
  cases easier to see.
- **Do not move JSON compatibility into the sidecar.** The sidecar already owns
  the only supported BARE behavior; adding negotiation there would create the
  exact unnecessary functionality this item removes.
- **Do not add production inverse conversions for test fakes.** Keep sidecar
  emulation under test directories and use generated protocol unions there.
- **Preserve binary coverage.** Several JSON browser tests currently avoid or
  swallow `ArrayBuffer` failures. BARE fixtures should strengthen these tests by
  asserting the binary result.
- **Preserve omission versus empty.** Typed config fixtures should set only the
  field being tested. Do not replace an omitted field with `{}`, `[]`, or a
  default object unless the test intentionally needs explicit presence.
- **Public API removal is intentional.** Downstream callers passing
  `payloadCodec`/`codec` will receive a type error; there is no functional
  replacement because the only supported wire is BARE.
- **No protocol schema/version regeneration is expected.** Production BARE bytes
  do not change. `serde_json`, generated BARE files, and lockfiles remain unless
  normal package metadata tooling proves otherwise.
- If runtime/core surfaces mirrored by `secure-exec` require regeneration, run
  `node scripts/generate-secure-exec-mirror.mjs` after the AgentOS revision;
  do not hand-edit the generated mirror.

## Intended one-item `jj` revision scope

Create exactly one dedicated stacked revision for Item 45, on top of the prior
completed item:

```text
refactor(protocol): remove legacy fixture codecs
```

The revision contains only this compatibility deletion/migration:

- Rust protocol: `crates/sidecar-protocol/src/{protocol,wire}.rs`, its protocol
  README, `crates/CLAUDE.md`, the listed native-sidecar test files, and
  `tests/support/mod.rs`;
- runtime core: the seven source/package/docs files above, deletion of
  `frame-payload-codec.ts` and its test, the three remaining focused test files,
  and the new test-only BARE fixture;
- runtime browser: the five source files above, the three runtime unit tests,
  fake sidecar support, real-WASM integration test, and browser harness fixture;
- browser/core: `packages/browser/src/converged-sidecar.ts`, its unit/worker
  fixtures, the two core native process tests, the core test-only BARE fixture,
  and `packages/core/CLAUDE.md`;
- `docs/thin-client-migration.md` and this research note for final status and
  evidence checkboxes.

Do not include unrelated runtime policy, protocol schema changes, generated
protocol rewrites, release versions, Item 46 presence work, or other pending
thin-client items in this revision.

## Proposed small diff sequence

Keep the single Item 45 revision, but build it in these reviewable stages:

1. Add and run the failing architecture guard; record the before inventories
   and focused compatibility-test results in the tracker.
2. Add typed Rust test helpers while the legacy parser still exists. Convert
   shared callers, then local helpers, then direct callers. Run each affected
   test binary as its caller group moves.
3. Convert `protocol.rs` and `generated_protocol.rs` to `WireFrameCodec`, then
   delete `NativeFrameCodec`, `NativePayloadCodec`, `legacy_test_config`, and
   the now-private dead parser functions/tests.
4. Add generated-BARE TypeScript fake-sidecar support and migrate runtime-core,
   runtime-browser, browser, and core tests before changing public options.
5. Delete JSON codec files/exports and codec selectors in one mechanical pass;
   update the protocol README and CLAUDE guidance in the same pass.
6. Run absence searches, the permanent architecture guard, focused Rust/TS
   suites, workspace checks, and expensive browser tests. Only then mark Item
   45 done and move its dedicated bookmark/revision forward.
