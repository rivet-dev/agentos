# Item 46 research — preserve omitted versus explicit Rust input

Status: implementation-ready research only, refreshed on 2026-07-14 against
working-copy revision `95aedc82` (`pzzlonpr`). This note does not modify
production code, tests, or the Item 46 tracker status.

## Recommendation

Make each Rust request option that maps to an optional TypeScript/protocol field
presence-aware, then forward the `Option` without inspecting its effective
value. The concrete collapses are:

1. VM `root_filesystem`, nested `disable_default_base_layer` / `lowers`, and
   `loopback_exempt_ports`;
2. process and shell `env`;
3. create/resume-session `env`, create-session `mcp_servers` and
   `skip_os_instructions`, plus optional fields inside MCP descriptors;
4. filesystem `recursive` for mkdir/delete;
5. actor-plugin DTOs that currently deserialize those fields into empty maps,
   empty lists, or `false` before calling the Rust client.

Use `Option<T>` at the Rust boundary. `None` means omitted; `Some(empty)` and
`Some(false)` mean exactly what the caller supplied. Do not add sidecar defaults
or change BARE/protocol types: all relevant wire fields are already optional.

Two TypeScript wire-boundary corrections are also required. ACP create must use
`skipOsInstructions: options?.skipOsInstructions ?? null`, and
`SidecarProcess.execute` must test `env`/`cwd` with `!== undefined`
instead of value truthiness. The high-level TS process client retains `{}` and
`""`, but the final runtime-core serializer currently drops them immediately
before encoding the request. TypeScript already preserves explicit
empty/default values for the other surfaces in scope.

Priority: **P1 overall** (**P1** actor-root behavior, **P2** remaining parity
collapses). Recommended-fix confidence: **high**. The conversion is a
mechanical `Option<T>` migration and every destination wire field is already
optional. The actor root bug is already observable: explicit
`rootFilesystem: {}` selects the actor's native durable root instead of the
requested overlay. Process/session env and filesystem recursion have the same
effective behavior for omitted and explicit empty/false values today, but
leaving them would retain the same proven representation bug and cross-client
mismatch.

## Original issue and observable impact

Rust uses value-bearing defaults where TypeScript uses optional fields, then
tries to infer presence by inspecting values:

```rust
root_filesystem != RootFilesystemConfig::default()
!loopback_exempt_ports.is_empty()
disable_default_base_layer.then_some(true)
!lowers.is_empty()
!env.is_empty()
skip_os_instructions.then_some(true)
recursive.then_some(true)
```

That inference is impossible. For example, these caller intents become the same
Rust value and absent wire payload:

```rust
AgentOsConfig::default()

AgentOsConfigBuilder::new()
    .root_filesystem(RootFilesystemConfig::default())
    .loopback_exempt_ports(vec![])
    .build()
```

TypeScript instead distinguishes `AgentOs.create({})` from
`AgentOs.create({ rootFilesystem: {}, loopbackExemptPorts: [] })`. Its root,
loopback, ACP env/MCP, and filesystem serializers already preserve that
distinction. The two TS exceptions found in this audit are ACP
`skipOsInstructions: false` and final process `env: {}` / `cwd: ""` encoding;
they are included below so Item 46 ends with actual wire parity.

The actor root is already an observable semantic failure. The actor installs its
durable `js_bridge` native root only when the caller omitted `rootFilesystem`,
but currently compares the resolved value to `RootFilesystemConfig::default()`.
Explicit `rootFilesystem: {}` is therefore mistaken for omission and replaced
with the actor database root.

Other sidecar defaults currently make omission and explicit empty/false behave
the same. They still must cross separately: the clients ship in lockstep, and a
future sidecar default must not silently affect only TypeScript.

## Current-stack code proof

The current stack still contains every collapse. These are implementation
anchors, not approximate subsystem references:

| Priority | File and symbol | Current collapse | Exact replacement |
|---|---|---|---|
| P1 | `crates/agentos-actor-plugin/src/config.rs:43-52`, `AgentOsConfigJson`; `:158`, `:171`, `to_agent_os_config`; `crates/agentos-actor-plugin/src/vm.rs:32-45`, `build_config` | JSON omission becomes `Vec::new`; explicit root `{}` becomes `RootFilesystemConfig::default()`, then the value comparison installs `js_bridge` | Keep `loopback_exempt_ports` and `root_filesystem` as `Option`; forward both; test the preserved `options.root_filesystem.is_none()` before installing the actor root |
| P1 | `crates/client/src/config.rs:19-34`, `AgentOsConfig`; `:77-94`, builder setters; `:650-675`, `RootFilesystemConfig` | Public Rust values erase presence before serialization | Use `Option<Vec<u16>>`, `Option<RootFilesystemConfig>`, `Option<bool>`, and `Option<Vec<RootLowerInput>>`; setters wrap caller input in `Some` |
| P1 | `crates/client/src/agent_os.rs:858-875`, `serialize_create_vm_config_for_sidecar`; `:892-929`, `serialize_root_filesystem_config_for_sidecar` | Equality/emptiness/boolean tests infer presence | Match the outer option; map nested options directly; never use default equality, `is_empty`, or `then_some(true)` to infer caller intent |
| P2 | `crates/client/src/process.rs:62-89`, `ExecOptions`; `:665-690`, `send_execute`; `:857-875`, `build_process_execute_request` | Empty env is forced to wire `None` | Carry `Option<BTreeMap<_, _>>` through all three symbols and map `Some(map)` to the wire map even when empty |
| P2 | `crates/client/src/shell.rs:41-49`, `OpenShellOptions`; `:127-135`, `open_shell` request | Empty env is forced to wire `None` | Make env optional and forward `options.env.map(...)` |
| P2 | `crates/client/src/session.rs:563-588`, `McpServerConfig` / `CreateSessionOptions`; `:613-620`, `ResumeSessionOptions`; `:1400-1434`, `create_session`; `:1460-1479`, `resume_session` | Empty env/MCP/nested MCP values and false skip flag are inferred as omitted | Make presence-bearing public fields optional; extract pure create/resume request projection helpers; serialize outer `Some([])` as `"[]"` and nested `Some(empty)` into that JSON |
| P2 | `crates/client/src/cron.rs:150-180`, `WireCreateSessionOptions` conversions | A present default options object is expanded to null/empty/false JSON fields | Mirror the optional session fields and add `skip_serializing_if = "Option::is_none"` |
| P2 | `crates/client/src/fs.rs:82-90`, `MkdirOptions` / `DeleteOptions`; `:309-352`, kernel request construction | Explicit false and omission both become wire `None` | Use `Option<bool>` and assign it directly to `GuestFilesystemCallRequest.recursive` |
| P2 | `crates/agentos-actor-plugin/src/actions/process.rs:17-68`; `actions/shell.rs:23-29,92-107`; `actions/session.rs:30-38,410-423`; `actions/filesystem.rs:35-36,113-121`; `actions/mod.rs:925-930` | Actor serde defaults erase the same presence before the Rust client sees it | Mirror the client's `Option` fields. Keep actor mkdir intentionally explicit with `Some(true)`; pass delete's optional bool through instead of `unwrap_or_default()` |
| P2 | `packages/runtime-core/src/sidecar-process.ts:1482-1494`, `SidecarProcess.execute` | Final TS serializer drops `{}` and `""` | Use `options.env !== undefined` and `options.cwd !== undefined` |
| P2 | `packages/core/src/agent-os.ts:2685-2700`, `AgentOs.createSession` | Explicit `skipOsInstructions: false` becomes ACP null/omitted | Use `options?.skipOsInstructions ?? null`; the existing env and MCP truthiness checks already retain `{}` and `[]` because both are truthy in JavaScript |

No protocol edit is needed. The target fields are already optional at
`crates/vm-config/src/lib.rs:13-45,263-281`,
`crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:342-391`, and
`crates/agentos-protocol/protocol/agent_os_acp_v1.bare:12-23,74-80`.

## Complete presence inventory

| Surface | Current Rust collapse | TS / wire behavior | Replacement |
|---|---|---|---|
| VM root descriptor | Root is always a default struct; serializer drops default descriptor | `rootFilesystem?:`; VM config uses `Option` | `Option<RootFilesystemConfig>`; retain explicit overlay `{}` |
| Root base-layer flag | `bool`; false becomes `None` | TS preserves false; VM config uses `Option<bool>` | `Option<bool>` copied directly |
| Root lowers | `Vec`; empty becomes `None` | TS preserves `[]`; VM config uses `Option<Vec<_>>` | Optional Vec; map `Some([])` to `Some([])` |
| VM loopback exemptions | `Vec`; empty becomes `None` | TS preserves `[]`; VM config uses `Option<Vec<u16>>` | Optional Vec copied directly |
| Exec/spawn env | Empty map becomes `None` | High-level TS retains `{}`, but `runtime-core::execute` drops it; Execute env is optional | Rust optional map plus TS `!== undefined` forwarding |
| PTY shell env | Same | Same final Execute collapse | Same |
| Exec/spawn/shell cwd | Rust already preserves `Some("")` | Final TS serializer drops `""` by truthiness | Keep Rust unchanged; TS uses `!== undefined` |
| Create/resume env | Empty map becomes ACP `None` | TS maps `{}` to an empty present Map | Optional map copied directly |
| Create MCP list | Empty Vec becomes ACP `None` | TS turns explicit `[]` into `Some("[]")` | Optional Vec; serialize whenever `Some` |
| MCP local args/env; remote headers | Empty nested values are skipped by serde | TS preserves explicitly supplied empty nested fields | Make each nested field optional; skip only `None` |
| Skip OS instructions | `bool`; false becomes ACP `None` | TS currently also collapses false; ACP uses `Option<bool>` | Rust `Option<bool>` and TS `?? null` |
| mkdir/delete recursive | `bool`; false becomes wire `None` | TS preserves `{ recursive: false }`; guest-fs wire uses `Option<bool>` | `Option<bool>` copied directly |
| Actor DTO mirrors | serde defaults optional fields before forwarding | Generated actor interfaces mark them optional | Mirror and forward the same `Option` types |

### Deliberate exclusions

Do not convert every Rust collection:

- `packages`, `mounts`, and `tool_kits` are consumed during atomic
  initialization. Both clients intentionally omit empty initialization lists,
  and a fresh VM has no prior collection to clear. TypeScript default packages
  remain the documented package-manager exception.
- process/shell argv is a required `ExecuteRequest.args` list.
- cron exec args are normalized to an empty list by the sidecar-owned cron
  model and have no distinct effective state.
- response collections/booleans are authoritative output, not configuration.
- mkdir/delete omission and false retain identical Linux behavior; presence is
  preserved for forwarding parity, not to invent behavior.

## What must remain client-side

Presence preservation does not move legitimate host-only responsibilities into
the sidecar:

- the TypeScript package manager may still select its documented default
  package list and forward those package paths; this is the sole default-list
  exception in the thin-client rule;
- callback closures and their local routing stay in the host client: process
  stdout/stderr handlers, cron callback closures, the one absolute alarm/wake
  hook, and the actor's durable `js_bridge` callback cannot be serialized;
- Rust public enums/builders may validate discriminants and serialize explicit
  caller input (`RootFilesystemKind`, native plugin descriptors, MCP variants),
  but must not resolve omitted runtime defaults;
- actor code may choose its host-backed durable root only when the public root
  option is actually absent. That host integration is not permission to treat
  an explicit `{}` as absence.

Everything else in this item is representation only. Do not add a client-side
effective-value getter, duplicate a sidecar default, or add a sidecar policy
branch merely to observe presence.

## Exact production edits

Current line anchors at working revision `95aedc82` (use the symbol names after
earlier stacked items shift line numbers):

| File | Current lines | Edit anchor |
|---|---:|---|
| `crates/client/src/config.rs` | 19–34, 77–94 | `AgentOsConfig`; builder loopback/root setters |
| `crates/client/src/config.rs` | 650–675 | `RootFilesystemConfig` fields/default |
| `crates/client/src/agent_os.rs` | 858–875 | `serialize_create_vm_config_for_sidecar` |
| `crates/client/src/agent_os.rs` | 892–945 | `serialize_root_filesystem_config_for_sidecar` |
| `crates/client/src/process.rs` | 62–89 | `ExecOptions` and `Default` |
| `crates/client/src/process.rs` | 665–690, 857–880 | `send_execute`; `build_process_execute_request` |
| `crates/client/src/shell.rs` | 41–49, 127–140 | `OpenShellOptions`; Execute request construction |
| `crates/client/src/session.rs` | 563–588, 613–620 | MCP/create/resume public types |
| `crates/client/src/session.rs` | 1400–1434, 1460–1479 | ACP create/resume request construction |
| `crates/client/src/cron.rs` | 150–180 | `WireCreateSessionOptions` and conversions |
| `crates/client/src/fs.rs` | 82–90, 309–352, 435–548 | option types, low-level request builders, public calls |
| `crates/agentos-actor-plugin/src/config.rs` | 28–56, 131–175 | JSON mirror and `to_agent_os_config` |
| `crates/agentos-actor-plugin/src/vm.rs` | 26–45 | actor root substitution |
| `crates/agentos-actor-plugin/src/actions/process.rs` | 17–70 | exec/spawn DTOs and conversion |
| `crates/agentos-actor-plugin/src/actions/shell.rs` | 23–31, 92–107 | shell DTO and conversion |
| `crates/agentos-actor-plugin/src/actions/session.rs` | 30–38, 410–423 | session DTO and conversion |
| `crates/agentos-actor-plugin/src/actions/filesystem.rs` | 35–36, 113–121 | explicit recursive call sites |
| `crates/agentos-actor-plugin/src/actions/mod.rs` | 925–930 | delete action's omitted-to-false conversion |
| `packages/core/src/agent-os.ts` | 2685–2701 | TypeScript ACP create request |
| `packages/runtime-core/src/sidecar-process.ts` | 1453–1507 | `SidecarProcess.execute` final request projection |
| `packages/agentos/src/actor.ts` | 132–157 | `buildConfigJson` omission/explicit-empty forwarding |

### 1. Rust VM configuration

In `crates/client/src/config.rs`, change the public config and builders:

```rust
pub loopback_exempt_ports: Option<Vec<u16>>,
pub root_filesystem: Option<RootFilesystemConfig>,

pub fn loopback_exempt_ports(mut self, ports: Vec<u16>) -> Self {
    self.config.loopback_exempt_ports = Some(ports);
    self
}

pub fn root_filesystem(mut self, root: RootFilesystemConfig) -> Self {
    self.config.root_filesystem = Some(root);
    self
}
```

Make the nested fields optional and have `Default` set both to `None`:

```rust
#[serde(default, rename = "disableDefaultBaseLayer",
        skip_serializing_if = "Option::is_none")]
pub disable_default_base_layer: Option<bool>,

#[serde(default, skip_serializing_if = "Option::is_none")]
pub lowers: Option<Vec<RootLowerInput>>,
```

Update the public comments at the same time: these fields do not have
client-owned defaults. Say that omission delegates to the sidecar/runtime; do
not leave `Default []`, `Default false`, or `Default overlay` wording on a
presence-bearing client field.

Keep `kind`, `mode`, and `native_plugin`: top-level presence distinguishes
omitted from `{}`, `mode`/plugin are already optional, and `kind` is the
client-only overlay/native selector.

In `crates/client/src/agent_os.rs`, replace the default comparison in
`serialize_create_vm_config_for_sidecar` with:

```rust
let (root_filesystem, native_root) = match config.root_filesystem.as_ref() {
    None => (None, None),
    Some(root) => {
        let (descriptor, native_root) =
            serialize_root_filesystem_config_for_sidecar(root)?;
        let descriptor = match root.kind {
            RootFilesystemKind::Overlay => Some(descriptor),
            RootFilesystemKind::Native =>
                (descriptor != vm_config::RootFilesystemConfig::default())
                    .then_some(descriptor),
        };
        (descriptor, native_root)
    }
};
```

An empty native descriptor may stay omitted because `nativeRoot` already
represents explicit native presence. Any explicitly supplied native mode,
base-layer flag, or empty lowers must remain in `rootFilesystem`.

Forward nested fields and loopback without value tests:

```rust
disable_default_base_layer: config.disable_default_base_layer,

lowers: config.lowers.as_ref().map(|lowers| {
    lowers.iter()
        .map(serialize_root_lower_config_for_sidecar)
        .collect::<Result<Vec<_>, _>>()
}).transpose()?,

loopback_exempt_ports: config.loopback_exempt_ports.clone(),
```

For native roots, reject only `Some(nonempty)` lowers; allow/preserve
`Some([])`. Keep mapping explicit ephemeral/read-only mode to
`nativeRoot.readOnly = Some(false/true)`.

Update direct VM/root literals in:

- `crates/client/src/agent_os.rs` tests;
- `crates/client/tests/agent_registry_e2e.rs`;
- `crates/client/tests/common/mod.rs`;
- `crates/client/tests/link_software_e2e.rs`;
- `crates/client/tests/loopback_probe_e2e.rs`;
- `crates/client/tests/mount_e2e.rs`;
- `crates/client/tests/native_root_mount_e2e.rs`;
- `crates/client/tests/os_instructions_e2e.rs`;
- `crates/client/tests/packages_aospkg_e2e.rs`;
- `crates/client/tests/pi_session_e2e.rs`;
- `crates/client/tests/session_e2e.rs`;
- `crates/client/tests/session_lifecycle_e2e.rs`;
- `crates/client/tests/shell_pty_packages_e2e.rs`.

### 2. Actor VM configuration

In `crates/agentos-actor-plugin/src/config.rs`, make
`loopback_exempt_ports: Option<Vec<u16>>`. `root_filesystem` is already optional;
forward it directly in `to_agent_os_config` and delete `.unwrap_or_default()`.

In `crates/agentos-actor-plugin/src/vm.rs`:

```rust
if options.root_filesystem.is_none() {
    options.root_filesystem = Some(RootFilesystemConfig {
        kind: RootFilesystemKind::Native,
        native_plugin: Some(...),
        ..Default::default()
    });
}
```

Do not compare with `RootFilesystemConfig::default()`. Explicit overlay `{}`
must prevent actor durable-root substitution; omission must retain it.

### 3. Process and shell environment

In `crates/client/src/process.rs`, change `ExecOptions.env` to
`Option<BTreeMap<String, String>>` and default it to `None`. Change
`send_execute` / `build_process_execute_request` to accept the option and use:

```rust
env: env.map(|entries| entries.into_iter().collect()),
```

If Item 43 has flattened `SpawnOptions`, make its final direct `env` optional;
do not restore the deleted `base: ExecOptions` wrapper.

In `crates/client/src/shell.rs`, make `OpenShellOptions.env` optional and forward
it with the same `map`, not `is_empty`.

In `packages/runtime-core/src/sidecar-process.ts`, preserve the value that
`packages/core/src/sidecar/rpc-client.ts::startTrackedProcess` already retained:

```ts
...(options.env !== undefined ? { env: options.env } : {}),
...(options.cwd !== undefined ? { cwd: options.cwd } : {}),
```

Do not use `Object.keys(env).length` or string truthiness to infer omission.
This single final serializer covers TypeScript exec, spawn, and PTY shell.
Command/entrypoint truthiness is Item 43's unsupported/divergent-option audit,
not a reason to widen Item 46 further.

Update process/shell literals in `process_e2e.rs`, `packages_aospkg_e2e.rs`,
`link_software_e2e.rs`, `shell_e2e.rs`, and `shell_pty_packages_e2e.rs`.

### 4. ACP session options and cron copies

In `crates/client/src/session.rs`:

```rust
pub struct CreateSessionOptions {
    pub cwd: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub mcp_servers: Option<Vec<McpServerConfig>>,
    pub skip_os_instructions: Option<bool>,
    pub additional_instructions: Option<String>,
}

pub struct ResumeSessionOptions {
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
}
```

Change MCP variants to optional nested fields:

```rust
Local {
    command: String,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
},
Remote {
    url: String,
    headers: Option<BTreeMap<String, String>>,
},
```

Use serde `default` plus `skip_serializing_if = "Option::is_none"`. Encode MCP
servers whenever the outer option is `Some`, including `Some(vec![])`. Do not
broaden this revision into the separately tracked Item 54 error-propagation
cleanup: changing the current `filter_map` behavior is owned there. Item 46
only changes whether explicitly supplied empty outer and nested fields remain
present.

Forward create/resume env and skip fields directly. Update direct literals in
`session_e2e.rs`, `os_instructions_e2e.rs`, and `pi_session_e2e.rs`.
Extract the create/resume field projection into small pure helpers if needed so
the presence matrix can be tested without booting a sidecar; those helpers must
only serialize, never resolve defaults.

In `crates/client/src/cron.rs`, give `WireCreateSessionOptions` the same optional
fields and `skip_serializing_if = "Option::is_none"` on every optional field.
Then `options: Some(Default::default())` serializes as `"options": {}` like TS,
instead of materializing null/empty/false fields. Keep both conversions lossless.

### 5. Filesystem recursion

In `crates/client/src/fs.rs`:

```rust
pub struct MkdirOptions { pub recursive: Option<bool> }
pub struct DeleteOptions { pub recursive: Option<bool> }
```

Have `kernel_mkdir` / `kernel_remove_path` accept `Option<bool>` and assign it
directly to `GuestFilesystemCallRequest.recursive`. Update `fs_e2e.rs` to wrap
explicit true/false values.

### 6. Actor action DTOs

Generated TypeScript actor interfaces already mark these fields optional, but
serde erases presence. Update:

- `actions/process.rs`: optional env on exec/spawn DTOs, forwarded directly;
- `actions/shell.rs`: optional env, forwarded directly;
- `actions/session.rs`: optional env and skip flag, forwarded directly;
- `actions/filesystem.rs`: wrap actor-supplied recursion in `Some(...)`.

Regenerate/check `packages/agentos/src/generated/actor-actions.generated.ts`.
It should remain byte-for-byte unchanged. Do not hand-edit generator output.

### 7. TypeScript parity corrections

In `packages/core/src/agent-os.ts`, replace:

```ts
skipOsInstructions:
    options?.skipOsInstructions === true ? true : null,
```

with:

```ts
skipOsInstructions: options?.skipOsInstructions ?? null,
```

No further TS production edit is needed. Existing code already preserves the
other explicit empty/default inputs. The process `env`/`cwd` correction belongs
in `packages/runtime-core/src/sidecar-process.ts` as described in section 3;
fixing only `rpc-client.ts` would be ineffective because that layer already
retains the values.

## Before and after tests

### Before behavior to record

Add focused regressions against the parent before production edits. The
minimum tracker proof should be a failing expectation, not only a
characterization that confirms two payloads are equal:

1. `agent_os.rs`:
   `create_vm_config_preserves_explicit_default_presence` constructs the config
   through `AgentOsConfigBuilder::root_filesystem(RootFilesystemConfig::default())`
   and `.loopback_exempt_ports(vec![])`, then expects
   `rootFilesystem: {}` and `loopbackExemptPorts: []`. It fails on the current
   parent because both keys are absent. A companion characterization may assert
   that this payload is currently identical to `AgentOsConfig::default()`.
2. `process.rs`: the current request builder maps an explicit empty env to
   `None`; omission cannot be represented.
3. `session.rs`: a pure ACP request builder shows empty env/MCP and false skip
   becoming `None`.
4. `fs.rs`: a pure guest-fs builder shows false recursive becoming `None`.
5. actor `config.rs`/`vm.rs`: omitted and explicit `{ rootFilesystem: {},
   loopbackExemptPorts: [] }` converge, and explicit `{}` is eligible for actor
   native-root replacement.
6. `packages/runtime-core/tests/sidecar-process.test.ts`, beside
   `preserves false, true, and omission for keepStdinOpen`: the final Execute
   payload currently omits explicit `env: {}` and `cwd: ""`. The same in-memory
   transport can capture mkdir/remove omission versus explicit false without a
   sidecar binary.
7. `packages/core/tests/session-route-registration.test.ts`: extend
   `createInjectedAgent` to retain the ACP request envelope and decode it with
   `decodeAcpRequest`; `skipOsInstructions: false` currently becomes null while
   explicit empty env/MCP remain present.

Use the parent result as before evidence, then replace collapse assertions with
the after matrix in the same item revision.

### After matrix

- `crates/client/src/agent_os.rs`:
  `create_vm_config_preserves_omitted_and_explicit_default_presence` checks
  default absent, explicit overlay `{}`, explicit false, empty lowers, and empty
  loopback.
- `crates/client/src/process.rs`: check env `None` versus `Some(empty)`.
- `crates/client/src/shell.rs`: extract a tiny pure request builder and check the
  same env matrix without filling PTY dimensions.
- `crates/client/src/session.rs`: create/resume request tests check absent,
  empty, and false; MCP exact JSON includes local `{ args: [], env: {} }` and
  remote `{ headers: {} }`.
- `crates/client/src/cron.rs`: extend
  `session_action_wire_shape_matches_typescript` with `options: {}` and explicit
  empty/false cases.
- `crates/client/src/fs.rs`: check recursive `None`, `Some(false)`, `Some(true)`.
- actor `config.rs`/`vm.rs`: omitted root gets actor `js_bridge`; explicit `{}`
  stays overlay; omitted/empty loopback remain distinct.
- retain `packages/core/tests/root-filesystem-descriptors.test.ts`'s
  `does not materialize omitted sidecar defaults` test as the TypeScript root
  reference: it already distinguishes `{}` from explicit false/empty fields.
- extend `packages/core/tests/session-route-registration.test.ts` at the
  injected ACP boundary: absent fields decode as null, explicit `{}` / `[]` /
  `false` decode as an empty map / `"[]"` / false. This is closer to the actual
  caller than the codec-only `agentos-protocol.test.ts` fixture.
- extend `packages/runtime-core/tests/sidecar-process.test.ts` at the final
  `SidecarProcess.execute` seam: absent env/cwd omit keys, `{}`/`""` retain
  keys, and a nonempty value remains unchanged. Use its existing
  `MemorySidecarTransport.requests` fixture. In the same file, call
  `mkdir`/`removePath` with omitted, false, and true recursion and inspect the
  captured `guest_filesystem_call` payloads.
- extend `packages/agentos/tests/actor.test.ts` so `buildConfigJson` omits absent
  root/loopback and retains explicit `{}` / `[]`.

Existing lower-layer tests cover adjacent behavior but not the collapse:

```text
cargo test -p agentos-vm-config \
  root_filesystem_preserves_omission_and_explicit_default_overrides
pnpm --dir packages/core exec vitest run \
  tests/root-filesystem-descriptors.test.ts --reporter=verbose
cargo test -p agentos-client --lib root_filesystem_serializer
```

The root tests cover sidecar defaults and nonempty/true descriptors; they do not
compare omission with explicit empty/false input. Record the failing parent
results from the seven before cases above in the tracker before replacing those
assertions with the after matrix.

Final focused gates:

```text
cargo fmt --all -- --check
cargo test -p agentos-client --lib
cargo test -p agentos-client --tests --no-run
cargo test -p agentos-vm-config
cargo test -p agentos-actor-plugin --lib
cargo test -p agentos-actor-plugin --test action_contract
cargo check -p agentos-client -p agentos-actor-plugin
pnpm --dir packages/core exec vitest run \
  tests/root-filesystem-descriptors.test.ts \
  tests/session-route-registration.test.ts \
  tests/agentos-protocol.test.ts
pnpm --dir packages/runtime-core exec vitest run \
  tests/sidecar-process.test.ts
pnpm --dir packages/agentos exec vitest run tests/actor.test.ts
pnpm --filter @rivet-dev/agentos-core check-types
pnpm --filter @rivet-dev/agentos-runtime-core check-types
pnpm --filter @rivet-dev/agentos check-types
```

When the sidecar binary is available, also run affected Rust filesystem,
loopback, process, shell, ACP session, OS-instructions, native-root, and session
lifecycle E2Es to protect existing nonempty/true behavior.

## Risks and safeguards

- **Public Rust break:** intentional `Option` migration. Update all literals in
  the same revision; do not add compatibility constructors that refill defaults.
- **Actor persistence:** omission must still install durable `js_bridge`; test it
  separately from explicit `{}`.
- **Native-root descriptor:** omit only an entirely empty descriptor when
  `nativeRoot` already carries explicit presence; retain explicit nested fields.
- **Item 43 overlap:** apply env presence to Item 43's final flattened spawn
  shape; do not restore removed options/wrappers.
- **Final TS serializer:** high-level request capture alone is insufficient;
  assert the runtime-core request immediately before `sendRequest` so an empty
  env or cwd cannot be dropped in a later adapter.
- **No default migration:** `None` always remains sidecar-owned resolution.
- **Generated churn:** protocol bindings and actor TS types should not change.
- **Item 54 boundary:** do not change the MCP `filter_map` error behavior in
  this revision. Item 46 preserves field presence; Item 54 owns conversion
  error propagation and host-visible warnings.

## Dependencies and sequencing

- Implement Item 46 after Items 37–45 in the requested stack. Item 43 is the
  only semantic overlap: it may remove or flatten process option fields, so
  apply `Option` to its final retained env shape rather than resurrecting
  deleted wrappers.
- Item 45 may move compatibility fixtures, but Item 46 must keep using the
  generated optional BARE fields and typed `agentos-vm-config`; do not restore a
  JSON compatibility codec to test presence.
- Actor cold-boot coverage from Item 40 is useful validation but not required to
  represent presence. Item 46's actor unit tests must run without conditionally
  skipping for a missing sidecar binary.
- No protocol/schema dependency exists: `CreateVmConfig`, `ExecuteRequest`, ACP
  create/resume, and guest-fs recursive fields are already optional. A schema or
  generated-binding diff signals scope drift.

## Proposed small diff sequence

Keep the dedicated Item 46 revision reviewable in this order:

1. Add the Rust and TypeScript presence matrices first and record the expected
   failures against the parent: actor explicit `{}` root, Rust VM/env/session/
   recursion presence, TS execute `{}` / `""`, and TS ACP false.
2. Change only the Rust public presence-bearing types and builder setters in
   `config.rs`, `process.rs`, `shell.rs`, `session.rs`, and `fs.rs`; mechanically
   update all struct literals until `cargo check -p agentos-client` is green.
3. Replace inference in the pure Rust wire projections. Extract only the small
   session/shell/fs request builders needed for unit tests; do not move policy or
   add effective defaults.
4. Mirror the new options through actor config/action DTOs. Fix the root choice
   using the outer `Option` preserved by `to_agent_os_config`; keep actor mkdir
   explicitly recursive with `Some(true)`.
5. Apply the two TS serializer corrections and extend the existing in-memory
   fixtures. No new transport abstraction or sidecar behavior is required.
6. Update cron's JSON mirror, run actor contract generation/checks, and confirm
   generated protocol and actor TypeScript surfaces are unchanged.
7. Run focused gates, then the full client/actor type and unit gates. Mark Item
   46 complete only after the tracker contains both the recorded failing-before
   evidence and passing-after commands.

## Bounded JJ revision

Create one dedicated stacked revision, for example:

```text
jj new -m "fix(client): preserve explicit default-valued input"
```

Expected production paths:

```text
crates/client/src/config.rs
crates/client/src/agent_os.rs
crates/client/src/process.rs
crates/client/src/shell.rs
crates/client/src/session.rs
crates/client/src/cron.rs
crates/client/src/fs.rs
crates/agentos-actor-plugin/src/config.rs
crates/agentos-actor-plugin/src/vm.rs
crates/agentos-actor-plugin/src/actions/process.rs
crates/agentos-actor-plugin/src/actions/shell.rs
crates/agentos-actor-plugin/src/actions/session.rs
crates/agentos-actor-plugin/src/actions/filesystem.rs
crates/agentos-actor-plugin/src/actions/mod.rs
packages/core/src/agent-os.ts
packages/runtime-core/src/sidecar-process.ts
```

Expected tests/call-site paths:

```text
crates/client/tests/common/mod.rs
crates/client/tests/agent_registry_e2e.rs
crates/client/tests/fs_e2e.rs
crates/client/tests/link_software_e2e.rs
crates/client/tests/loopback_probe_e2e.rs
crates/client/tests/mount_e2e.rs
crates/client/tests/native_root_mount_e2e.rs
crates/client/tests/os_instructions_e2e.rs
crates/client/tests/packages_aospkg_e2e.rs
crates/client/tests/pi_session_e2e.rs
crates/client/tests/process_e2e.rs
crates/client/tests/session_e2e.rs
crates/client/tests/session_lifecycle_e2e.rs
crates/client/tests/shell_e2e.rs
crates/client/tests/shell_pty_packages_e2e.rs
packages/core/tests/root-filesystem-descriptors.test.ts
packages/core/tests/session-route-registration.test.ts
packages/core/tests/agentos-protocol.test.ts
packages/runtime-core/tests/sidecar-process.test.ts
packages/agentos/tests/actor.test.ts
docs/thin-client-migration.md
```

Do not include protocol schema/generated-binding or sidecar-policy changes. If
actor generation only reorders text, revert that generated churn.
