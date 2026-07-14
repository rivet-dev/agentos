# Item 51 research: align active guidance with the sidecar architecture

Status: implementation-ready research only, revalidated 2026-07-14 through
working-copy change `sqnqyqwsupmt` (Item 81). Item 38 is a sealed ancestor and
its verifier/CI wiring are present. Item 50 is still pending and must be sealed
before Item 51 edits the adjacent package-descriptor comments. This note does
not modify production code, tests, or the Item 51 tracker status.

## Recommendation

Make Item 51 one documentation-truth revision, stacked after Items 38 and 50.
Correct four classes of false claims and extend the claim verifier introduced by
Item 38:

1. the native SDK embeds the VM/kernel in-process;
2. runtime packages are unpacked directories whose JSON manifests are read at VM
   startup;
3. omitted AgentOS permissions deny access; and
4. a guest `agentos-software link` command exists.

The replacement model is:

- TypeScript and Rust are transport clients for a shared sidecar process;
- the normal package input is one packed `.aospkg` path, with an unpacked
  directory supported only as a local transition path;
- the `.aospkg` chunk-1 vbare manifest is the only runtime manifest;
- the sidecar projects package payloads at
  `/opt/agentos/pkgs/<name>/<version>`, a `current` symlink under that package,
  and command symlinks under `/opt/agentos/bin`;
- `agentos-package.json` is source/toolchain input consumed and stripped by the
  packer, not a guest/runtime manifest;
- live linking is a host SDK request (`linkSoftware` / `link_software`) forwarded
  to the sidecar, not a privileged guest CLI; and
- omitted AgentOS permissions select the sidecar-owned allow-all product
  default, while a directly constructed generic kernel remains deny-all.

Do not add a compatibility manifest, package scanner, guest link command, or
client-side correction for any of these docs. No runtime behavior needs to move
for this item.

Priority: **P2**. Fix confidence: **high**. The current package container,
projection, link RPC, process boundary, and permission normalization all have
direct code and test evidence. The path inventory is also high confidence after
an explicit source/public scan; the important guard is that checked
`website/public/docs` copies are independent inputs and must be updated
explicitly.

## Current-stack revalidation: exact patch map

All four tracker claim classes still have active contradictions at
`sqnqyqwsupmt`. Item 38 corrected the public permission pages, but it did not
remove the contradictory native-sidecar test instruction in `crates/CLAUDE.md`;
Item 50 is not implemented yet. These are the exact active anchors the Item 51
revision must change. Line numbers are current-stack anchors, not stable
identifiers; match the quoted text when applying the patch.

| Claim class | Exact current contradictory text | Replace with |
|---|---|---|
| Native runtime is in the SDK process | `README.md:16,24,135`; `examples/core/README.md:10,16-17`; `website/src/content/docs/docs/core.mdx:34`; `website/src/content/docs/docs/versus-sandbox.mdx:7,15`; `website/public/docs/docs/versus-sandbox.md:5,13` say “runs inside your process,” “no client/server split,” “boots ... in-process,” or “in-process operating system kernel.” | The TypeScript/Rust API is an in-process handle, but it spawns or reuses a **separate shared local sidecar process**. VMs are sidecar-owned kernel state plus executors; there is no container or OS process per VM. Do not claim “nothing executes on the host”; the trusted sidecar is a host process and mediates the untrusted guest. |
| Packed packages ship/read JSON manifests | `website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx:3,63-113` and public copy `:3,35-85`; `website/public/docs/docs/custom-software/definition.md:3,7,84-85,95-124`; `registry/README.md:16-39`; `registry/CONTRIBUTING.md:20-30`; `packages/agentos-toolchain/README.md:10-25,30-55,74-76`; source/API comments in `packages/agentos-toolchain/src/manifest.ts:4-10`, `packages/manifest/src/index.ts:36-79`, `packages/core/src/agentos-package.ts:1-12`, `packages/core/src/packages.ts:5-18`, `packages/core/src/types.ts:48-53`, `packages/core/src/agent-os.ts:2677-2680`, `packages/agentos/src/actor.ts:145-147`, `crates/client/src/config.rs:20-25`, `crates/client/src/agent_os.rs:402-406`, and `crates/client/src/session.rs:1405-1407` describe a package directory, projected `agentos-package.json`, or runtime JSON parsing. | The normal artifact is one `.aospkg`: 16-byte header + versioned vbare `PackageManifest` + vbare `MountIndex` + mount tar. Chunk 1 is the only runtime manifest. `agentos-package.json` is source/toolchain input, compiled into chunk 1 and stripped. A directory is a local transition input only. Projection is `/opt/agentos/pkgs/<name>/<version>`, `current`, and `/opt/agentos/bin/<command>`. |
| AgentOS omission denies permissions | `crates/CLAUDE.md:138` says `permissions: None` is default-deny for native-sidecar helpers, contradicting the already-correct two-layer rule at `crates/CLAUDE.md:46-51`, `packages/core/CLAUDE.md:121`, and the Item 38 public docs. | Direct generic `KernelVmConfig` omission remains deny-all. An AgentOS `CreateVm` request omission is normalized by the sidecar to allow-all, including omitted top-level domains in a partial product policy. Native-sidecar product tests may omit policy unless testing restrictions; direct kernel tests must opt into allow-all when broad access is needed. |
| A guest registry command performs live linking | Package architecture source `:152-163,218-229` and public copy `:109-120,171-182` document `agentos-software link <path>`, writable-layer symlinks, and snapshot persistence. | Delete the guest CLI section. Document host `vm.linkSoftware({ packagePath: "/.../package.aospkg" })` / Rust `link_software(PackageDescriptor { path })`. Clients forward one path; the sidecar validates and adds read-only package/current/command leaf mounts to the live VM. The dynamic link lasts for that live VM and is not serialized by filesystem-layer snapshots. |

The active agent-instruction cleanup remains part of this item because it would
otherwise keep directing future changes back toward the deleted architecture:

- `crates/CLAUDE.md:8` says V8 execution is “CURRENTLY BROKEN”; `:26-27,
  52,54-55,78-90,99,102-142` contains many deleted `crates/sidecar` /
  `crates/sidecar-browser` paths, including the permission contradiction above.
- `crates/execution/CLAUDE.md:15-74` retains the historical recovery checkout,
  host-Node fallback table, and obsolete allowlist path; `:90,107,117,131-132,
  170,175,195,198-199` retains deleted paths or the deleted
  `packages/secure-exec-core` build command.
- `packages/core/CLAUDE.md:5,9` says the Node runtime is broken and the client
  wraps the kernel; `:39,103,201` names a deleted protocol/test path and the
  wrong logging owner/knob. Its thin-client rules at `:10-35,54-59` are already
  correct and must be preserved.
- Root `CLAUDE.md:51-78` is authoritative and already says `.aospkg`/vbare,
  `/opt/agentos/pkgs`, sidecar-owned resolution/defaults, omission preservation,
  and the sole TypeScript package-manager exception. **Do not rewrite it.** Add
  required verifier claims so removal or contradiction fails CI. `AGENTS.md` is
  a symlink to this file and must not be edited separately.

### Direct sources that justify the replacement wording

| Behavior | Current source/test anchor |
|---|---|
| Native clients own a child/shared sidecar transport | `crates/sidecar-client/src/transport.rs:542-620`; `crates/client/src/sidecar.rs:1-5,27-30,144-161`; TypeScript shared-child implementation at `packages/core/src/agent-os.ts:3220-3421` |
| Package bytes and sole runtime manifest | `crates/vfs/package-format/v1.bare:1-25,73-84,119-146`; `crates/vfs/src/package_format/pack.rs:1-12,23-24`; `packages/agentos-toolchain/src/aospkg.ts:1-15,99-130` |
| Packed file is normal; directory is transition-only | `crates/native-sidecar/src/package_projection.rs:349-380`; `crates/client/src/agent_os.rs:111-119` |
| Sidecar constructs package/current/command leaf mounts | `crates/native-sidecar/src/package_projection.rs:390-455`; command targets explicitly use `../pkgs/<name>/current/...` at `:449-454` |
| Live link is a sidecar mutation, not a guest command | `crates/native-sidecar/src/vm.rs:662-789`; TypeScript forwarding call at `packages/core/src/agent-os.ts:2127-2150`; Rust forwarding call at `crates/client/src/agent_os.rs:402-429` |
| Product omission is allow-all | `crates/native-sidecar/src/vm.rs:186-200`; `crates/native-sidecar-core/src/permissions.rs:57-73` |
| Generic kernel omission is deny-all | `crates/kernel/src/permissions.rs:164-180`; `crates/kernel/tests/default_deny_guards.rs:112-145` |
| Packed JSON is absent from the guest projection | `crates/client/tests/packages_aospkg_e2e.rs:72-113` |

### Exact verifier mechanics for the four claims

Extend Item 38's existing verifier rather than creating another script:

1. Add a fixed `guidanceFiles` inventory for the CLAUDE, README, registry,
   toolchain, example/snippet, and exported-comment files. Keep the two website
   trees recursive. Missing fixed files must emit `required-guidance-file`.
2. Keep permission rules sentence- and path-scoped. A global ban on
   “deny-by-default” would incorrectly reject the generic-kernel invariant.
3. Add path-scoped prose rules for `runtime-in-process`,
   `runtime-no-boundary`, `package-runtime-json`,
   `package-directory-primary`, `package-old-root`, and
   `package-dir-public-api`.
4. Add a `deleted-software-cli` rule that scans **raw lines including fenced
   code**. The current verifier calls `stripFencedCode`; if the rule uses only
   that prose view, the literal `agentos-software link <path>` example can evade
   the check after its prose cross-reference is removed. A per-rule
   `includeFences: true` switch is the smallest implementation.
5. Add positive required claims for the shared sidecar, `.aospkg`/vbare/JSON
   stripping, `/opt/agentos/pkgs`, host `linkSoftware`, and root thin-client
   invariants. This prevents deleting stale paragraphs from being sufficient.
6. In `scripts/verify-thin-client-docs.test.mjs`, key `requiredContent` by full
   repository-relative path. The current helper guesses the website root from
   a suffix and cannot correctly materialize new CLAUDE/registry/source-comment
   fixtures.

New focused fixture cases must cover all four stale classes, a missing root
thin-client claim, JSON described legitimately as toolchain input, a legitimate
transition-directory statement, a legitimate sidecar-local “in-process limit
registry,” and source/public website copies. Include the newly discovered
`packages/agentos/src/actor.ts:145-147` stale runtime-JSON comment in both the
fixed inventory and Item 51's edit set.

## Authoritative implementation evidence

| Claim | Source of truth |
|---|---|
| `.aospkg` is header + vbare manifest + mount index + mount tar | `crates/vfs/package-format/v1.bare:1-25` |
| Chunk 1 is the only runtime manifest; JSON is stripped | `crates/vfs/package-format/v1.bare:22-25`, `crates/vfs/src/package_format/pack.rs:1-8`, `packages/agentos-toolchain/src/aospkg.ts:1-15` |
| Package/command projection paths | `crates/vfs/package-format/v1.bare:73-84,119-141`, `crates/native-sidecar/src/package_projection.rs:390-455` |
| Normal input is a packed file; a directory is transitional | `crates/native-sidecar/src/package_projection.rs:349-380`, `crates/client/src/agent_os.rs:111-119` |
| TypeScript link is a thin sidecar request | `packages/core/src/agent-os.ts:2127-2150` |
| Rust link is a thin sidecar request | `crates/client/src/agent_os.rs:402-429` |
| Sidecar owns live package mounts, commands, and agent state | `crates/native-sidecar/src/vm.rs:662-779` |
| Native clients spawn/reuse a sidecar process | `crates/sidecar-client/src/transport.rs:573-615`, `crates/client/src/sidecar.rs:27-30,144-161` |
| Omitted AgentOS permission policy is allow-all | `crates/native-sidecar/src/vm.rs:195-200`, `crates/native-sidecar-core/src/permissions.rs:57-73` |
| Generic kernel construction is deny-all | `crates/kernel/src/permissions.rs:164-170`, `crates/kernel/tests/default_deny_guards.rs:114-139` |
| Limit tracking/logging belongs to AgentOS and uses the real knob | `crates/bridge/Cargo.toml:2`, `crates/kernel/src/resource_accounting.rs:6`, `crates/native-sidecar/src/main.rs:1-13` (`agentos_bridge`, `AGENTOS_LOG`, stderr) |

The packed-package E2E also proves the exact externally visible result:
`crates/client/tests/packages_aospkg_e2e.rs:72-113` checks that commands exist
under `/opt/agentos/bin` and that
`/opt/agentos/pkgs/coreutils/current/agentos-package.json` does not exist.

## Original issue and current false claims

### 1. Active agent instructions describe a broken, deleted architecture

`crates/CLAUDE.md:8` first describes V8 correctly, then says the execution
engine is “CURRENTLY BROKEN,” spawns host Node, and must be recovered from an
external secure-exec checkout. That is false: the live path uses the shared V8
runtime. The same file contains many references to deleted
`crates/sidecar/...` and `crates/sidecar-browser/...` paths, and line 138 says
`permissions: None` means default-deny in sidecar tests.

`crates/execution/CLAUDE.md:17-74` combines a correct current-state sentence
with a historical recovery checklist and a “current reality” table that still
says builtins fall through to real host Node. The table contradicts both the
preceding current-state paragraph and the current V8 bridge/conformance suites.
The remainder of the file repeatedly points at deleted `crates/sidecar` and
`packages/secure-exec-core` paths.

`packages/core/CLAUDE.md` has the same contradictions:

- line 5 says Node execution is currently broken;
- line 9 says the SDK wraps the kernel and proxies it directly;
- line 39 points at the deleted
  `crates/sidecar/protocol/agentos_native_sidecar_v1.bare`;
- line 103 points tests at `crates/sidecar/tests`; and
- line 201 attributes AgentOS queue tracking and logging to secure-exec and
  documents an unimplemented `SECURE_EXEC_LOG` knob.

The root `CLAUDE.md:47-69` is already correct and should not be rewritten. It
clearly says clients are thin transports, omissions stay omitted, the sole
TypeScript exception is package-manager default selection, package resolution
is sidecar-owned, and `agentos-package.json` is pack-time input. Item 51 should
make the verifier require those claims so deletion cannot make stale guidance
look acceptable.

### 2. Published docs say the sidecar is in-process

The following active text contradicts the actual child/shared-sidecar process:

- `README.md:16,24,135` — “runs inside your process,” “in-process operating
  system kernel,” and “nothing executes on the host”;
- `examples/core/README.md:10,16` — “no client/server split” and “boots a VM
  in-process”;
- `website/src/content/docs/docs/core.mdx:34` — the same claim immediately
  before lines 38-46 correctly describe the shared sidecar process; and
- `website/src/content/docs/docs/versus-sandbox.mdx:7,15` plus
  `website/public/docs/docs/versus-sandbox.md:5,13` — “runs inside your
  process.”

The correct distinction is not “process versus VM.” The trusted client spawns
or reuses a local sidecar process; each VM is lightweight sidecar-owned kernel
state plus its executors, rather than a full container or one host process per
VM. Guest code remains isolated and resource access is mediated even though the
trusted sidecar itself necessarily runs on the host.

### 3. Package docs teach the transition directory as the runtime format

The largest stale surface is
`website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx`
and its checked public Markdown copy. Current lines 3, 63-113 say:

- a package is a directory/plain npm dependency;
- `package.json` and `agentos-package.json` are projected into the guest;
- the sidecar reads the JSON on mount;
- commands are discovered from runtime `package.json`;
- clients forward a package directory; and
- package roots omit the required `pkgs` component.

The page's linking section at lines 115-168 also describes package links as
writable-layer filesystem entries with snapshot persistence. In the live
implementation they are sidecar-managed package leaf mounts. A live
`LinkPackage` updates `VmConfiguration.mounts`, command maps, driver
registration, and projected agent launch state; `ExportSnapshot` exports a
filesystem layer, not external package descriptors. Therefore the docs must
promise live-VM duration only, not persistence of a dynamic package reference
through a layer snapshot/recreated VM.

Other active package guidance repeats the old model:

- `packages/agentos-toolchain/README.md:10-25,30-55,74-76` says secure-exec owns
  packaging, the output is a directory with no JSON, and consumers use a
  directory descriptor. The implementation actually writes
  `agentos-package.json`, packs it into vbare chunk 1, strips it from the mount
  tar, and emits a sibling `.aospkg`.
- `registry/README.md:1-50` says packages export `packageDir`, project under
  `/opt/agentos/<name>/<version>`, and ship JSON in `dist/package/` as runtime
  metadata.
- `registry/CONTRIBUTING.md:20-30` calls `agentos-package.json` the runtime
  manifest.
- `website/public/docs/docs/custom-software/definition.md` is an old
  `packageDir` version of the now mostly corrected source MDX.
- `website/public/docs/docs/custom-software/building-wasm.md` still names the
  secure-exec repository, the deleted `make copy-wasm` lifecycle, and
  `{ packageDir }`.
- the source `definition.mdx:50-60,74-87` still documents `pack` as producing a
  directory/current-symlink layout even though the CLI default is
  `<name>-package.tar` plus `<name>-package.aospkg`.
- `definition.mdx:139,147,153` and `publishing.mdx:10` omit `/pkgs` from the
  projected path.
- `agents/custom.mdx:66,87,93` and its public copy describe the source JSON as
  though it remains in the packaged runtime.

The code snippets linked from the custom-software page also point at transition
directories rather than the normal artifact:

- `examples/software/quickstart-node/my-tool.ts:5-9`;
- `examples/software/quickstart-wasm/my-cmds.ts:5-6`; and
- `examples/software/quickstart-agent/my-agent.ts:5-10`.

### 4. A deleted guest registry command is documented as public API

`website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx:156,218-230`
and public-copy lines 113,171-182 document:

```text
agentos-software link <path>
```

There is no `agentos-software` command package or sidecar tool handler. The real
interfaces are the host SDK methods above, which emit `LinkPackageRequest` and
let the sidecar parse/project the package.

### 5. Exported doc comments repeat the stale package model

These are part of generated API guidance even though they live beside code:

- `packages/agentos-toolchain/src/manifest.ts:4-10` calls the JSON fields
  runtime metadata without explaining that the copied `dist/package` JSON is a
  pack intermediate compiled into chunk 1 and stripped;
- `packages/manifest/src/index.ts:36-79` calls JSON the sidecar-read runtime
  manifest and describes a string migration reference plus directory
  descriptor as the runtime package surface;
- `packages/core/src/agentos-package.ts:1-12` says the public value is a package
  directory and JSON is runtime metadata;
- `packages/core/src/packages.ts:5-18` describes only package directories;
- `packages/core/src/types.ts:49-54` says agent lookup reads projected JSON;
- `packages/core/src/agent-os.ts:2127-2150,2677-2683` describes a
  staging directory, snapshot persistence, and projected JSON;
- `packages/agentos/src/actor.ts:145-147` says the actor forwards package
  directories and that the sidecar resolves projected JSON;
- `crates/client/src/config.rs:20-25` says all packages are host directories
  with JSON;
- `crates/client/src/agent_os.rs:402-406` says live linking appends to a staging
  directory; and
- `crates/client/src/session.rs:1405-1408` says agent resolution reads projected
  JSON.

Item 50 removes the deprecated TypeScript string descriptor before Item 51.
Update the comments against its resulting `{ packagePath }`-only surface; do not
reintroduce the deleted alias while correcting prose.

### 6. Active observability and registry pages still name the compatibility mirror as owner

The initial Item 51 inventory missed three source/public pairs that still teach
secure-exec as the current implementation:

- `website/src/content/docs/docs/architecture/limits-and-observability.mdx:15-23,73-77`
  and public lines `13-21,71-75` call the kernel, sidecar, and logging
  “secure-exec,” even though the live crate is `agentos-bridge`, the native
  sidecar owns the queues, and `AGENTOS_LOG` is the real stderr log knob;
- `website/src/content/docs/docs/resource-limits.mdx:93-95` and public lines
  `44-46` repeat the secure-exec logging owner; and
- `website/src/content/docs/docs/software.mdx:33` and public line `25` send
  contributors to the obsolete secure-exec registry repository.

Replace the owner with AgentOS/the AgentOS native sidecar and link registry
contributors to this repository. Preserve “in-process limit registry” on the
limits page: that phrase accurately describes a registry inside the sidecar and
is not a claim that the SDK client embeds the VM.

## Exact edits

### Agent instructions

#### `crates/CLAUDE.md`

1. At the Node/V8 overview, delete only the false “CURRENTLY BROKEN” and recovery
   text. End the bullet after the current V8/kernel-backed description.
2. Remove the historical deleted-JS-kernel recovery prescription from invariant
   4; retain the rule that guest builtins must be kernel-backed or denied.
3. Mechanically update real native paths:
   `crates/sidecar/src` -> `crates/native-sidecar/src`,
   `crates/sidecar/tests` -> `crates/native-sidecar/tests`, and
   `crates/sidecar-browser/src` -> `crates/native-sidecar-browser/src`.
   Do not change `crates/agentos-sidecar` references to native-sidecar: that is
   the ACP extension crate, not a rename.
4. Replace the command-discovery bullets at current lines 80-82 with the stable
   architecture: transition command roots under `/__secure_exec/commands` may
   still be discovered by `bootstrap.rs`, while packed package commands come
   from the vbare manifest and are registered at `/opt/agentos/bin`. Do not
   describe the transition root as the package source of truth or freeze the
   current runtime-driver builtin list into agent guidance.
5. Replace line 138 with the two-layer rule: direct `KernelVmConfig` omission is
   deny-all; AgentOS sidecar request omission is normalized to allow-all before
   the kernel is built. Native-sidecar product tests may omit policy unless they
   are testing restrictions; direct kernel tests must opt into allow-all when
   needed.

#### `crates/execution/CLAUDE.md`

Replace the historical block from `## Node.js Isolation Model` through the
obsolete “Current reality vs required state” table with a compact current-state
section:

```text
Guest JavaScript runs only in shared V8 isolate sessions. Builtins resolve
through the checked bridge/polyfill registry and kernel/sidecar RPC; an
unsupported builtin is denied, never delegated to a guest host-Node fallback.
The conformance sources are crates/execution/tests/javascript_v8.rs,
crates/native-sidecar/tests/builtin_conformance.rs, and
crates/native-sidecar/tests/builtin_completeness.rs.
```

Delete the external recovery checkout/file list and the per-builtin gap table;
they are historical task notes, not durable agent instructions. In the retained
rules, update `crates/sidecar/...` to `crates/native-sidecar/...`, update
sidecar test paths likewise, and replace
`pnpm --dir packages/secure-exec-core build:v8-bridge` with
`pnpm --dir packages/build-tools build:v8-bridge`.

#### `packages/core/CLAUDE.md`

- Replace line 5 with the already-true isolation invariant and no “currently
  broken” suffix.
- Replace “wraps the kernel” at line 9 with “is a thin transport handle to a
  sidecar-owned VM.” Preserve lines 10-15, which are the requested client
  simplicity rule.
- Point ACP schema ordering guidance at
  `crates/agentos-protocol/protocol/agent_os_acp_v1.bare` and the generated
  TypeScript `src/sidecar/agentos-protocol.ts`.
- Change native tests from `crates/sidecar/tests/...` to
  `crates/native-sidecar/tests/...`.
- At line 201, use `agentos_bridge::queue_tracker`, call the component AgentOS,
  retain the stderr/structured-warning rule, and replace the nonexistent
  `SECURE_EXEC_LOG` with the implemented native-sidecar `AGENTOS_LOG` knob
  (`crates/native-sidecar/src/main.rs:1-13`).

The root `CLAUDE.md` needs no prose edit. Add positive verifier assertions for
its thin-client, omission, sidecar-owned package/session/default, and TypeScript
package-manager-exception statements.

### Runtime/process public docs

- `README.md`: replace lines 16 and 24 with “embeds into your backend and
  launches/reuses a shared local sidecar process”; replace line 135 with the
  client -> sidecar -> kernel/executor trust boundary. Do not touch the package
  inventory table (Item 55 owns it). Item 38 owns lines 18 and 128.
- `examples/core/README.md`: say there is no actor requirement, but the
  `AgentOs` handle communicates with a shared sidecar; change “boots in-process”
  to “creates a VM in the shared sidecar.”
- `website/src/content/docs/docs/core.mdx`: make the boot paragraph agree with
  its existing “Sidecar process” section. The direct API is not evidence of no
  client/server/process boundary.
- `website/src/content/docs/docs/versus-sandbox.mdx` and its public copy: use
  “shared local sidecar; no container per VM” in the intro/cost row. Item 38
  owns the permission row.
- `website/src/content/docs/docs/architecture/networking.mdx` and its public
  copy: update only the deleted crate paths to `crates/native-sidecar`; retain
  the currently implemented `net.http_request` host-to-guest/loopback mechanism
  rather than turning this docs item into a networking refactor.
- `website/src/content/docs/docs/architecture/limits-and-observability.mdx` and
  its public copy: name the AgentOS kernel/native sidecar/client layers, keep
  `agentos_bridge::queue_tracker` as the in-sidecar registry, and name
  `AGENTOS_LOG` plus stderr as the logging contract.
- `website/src/content/docs/docs/resource-limits.mdx` and its public copy:
  replace only the secure-exec logging owner with the AgentOS native sidecar;
  retain the already-correct `AGENTOS_LOG` values and limit behavior.
- `website/src/content/docs/docs/software.mdx` and its public copy: point the
  registry-source link at this AgentOS repository's `registry/` tree.

### Package architecture page

In both package-and-command-resolution copies, replace the package and linking
sections with this exact conceptual content:

```text
The normal runtime package is a packed `.aospkg` file. Its layout is a 16-byte
header, a versioned vbare `PackageManifest`, a versioned vbare `MountIndex`, and
an uncompressed mount tar. The vbare manifest is the runtime source of truth.
`agentos-package.json` and the source `package.json` `bin` map are pack-time
inputs used to build that manifest. `agentos-package.json` is stripped from the
mount tar. `package.json` may remain in the payload for Node module resolution,
but runtime command discovery never parses it. A directory path is accepted
only as a local transition input and is converted by the sidecar into the same
descriptor model.

/opt/agentos/
├── bin/<command> -> ../pkgs/<name>/current/<manifest command entry>
└── pkgs/<name>/
    ├── current -> <version>
    └── <version>/                 # read-only tar/package projection
        └── <package payload>      # no agentos-package.json in packed packages
```

State explicitly that startup reads chunk 1 without scanning the tar or parsing
guest JSON. Commands are derived at pack time and stored as
`PackageManifest.commands`; normal `$PATH`/header dispatch remains authoritative
after the sidecar creates the virtual links.

Replace the runtime-install/writable-layer/persistence prose and the complete
`agentos-software` section with a host API section:

```ts
await vm.linkSoftware(myPackage); // { packagePath: "/.../package.aospkg" }
```

Explain that the client forwards the path, the sidecar validates the package
and adds its read-only package/current/command mounts to the live VM, and a
duplicate command is rejected. The dynamic link lasts for that live VM; callers
must provide packages again when creating/restoring a new VM because filesystem
layer snapshots do not serialize external package references.

Keep the Linux `$PATH`, shebang/binfmt, shadowing, and permission-policy sections
unless their claims independently conflict with the current runtime.

### Package authoring/registry guidance

#### `packages/agentos-toolchain/README.md`

Rewrite around the actual CLI:

- AgentOS, not secure-exec, owns the package format and registry.
- `pack` emits `<out>.tar` and a sibling `<out>.aospkg`; `.aospkg` is the normal
  runtime artifact.
- the temporary clean directory contains `package.json`,
  `agentos-package.json`, `bin`, and the dependency closure;
  `agentos-package.json` is consumed into chunk 1 and stripped;
- the sidecar consumes `{ packagePath }` and projects under `/opt/agentos/pkgs`;
  and
- use current `crates/native-sidecar` / `crates/vfs` paths.

Update `packages/agentos-toolchain/src/cli.ts:13-17,130-133` so `pack` help and
success output name the tar intermediate **and** `result.packageAospkg`, the
sibling runtime artifact. Correct the stale `PackResult.packageAospkg` comment
at `src/pack.ts:48-54` (the current implementation always emits it at lines
433-440). Update only stale comments/paths in `src/build.ts` and `src/pack.ts`
(`packagePath`, `/opt/agentos/pkgs`, embedded manifest); do not rename internal
`packageDir` variables, which correctly name pack-time working directories.

#### `registry/README.md` and `registry/CONTRIBUTING.md`

Use “AgentOS Registry.” A published package exports `{ packagePath }` pointing
at `dist/package.aospkg`; npm excludes the `dist/package/` and `.tar`
intermediates. Describe source `agentos-package.json` as toolchain/registry
metadata compiled into the embedded runtime manifest. Show the projected path
as `/opt/agentos/pkgs/<name>/<version>`. Replace the old sibling-repository
workflow with the current in-repository workspace/`just registry-*` workflow.

#### Website custom-software pages and snippets

- Bring `website/public/docs/docs/custom-software/definition.md` forward to the
  source page's `{ packagePath }`/`.aospkg` model, then apply the remaining
  source corrections below.
- In source/public definition docs, make the default `pack` example point to
  `<name>-package.aospkg`, call JSON the source manifest, and change every
  projected package path to `/opt/agentos/pkgs/<name>`.
- Update the three `examples/software/quickstart-*/*.ts` snippet sources to
  resolve a `.aospkg` path. WASM authoring should run `agentos-toolchain build`
  and use `dist/package.aospkg`; a bare directory remains documented only as a
  local transition option in the reference section.
- In `website/src/content/docs/docs/agents/custom.mdx` and its public copy, say
  the source JSON's agent block is compiled into the packed manifest. Keep the
  existing `{ packagePath }` source example and update the public copy from
  `packageDir`.
- In `website/src/content/docs/docs/custom-software/building-wasm.mdx`, replace
  secure-exec repository links/names with this AgentOS repository. Refresh its
  public copy from the current `just registry-*`, `{ packagePath }`, and
  `.aospkg` source guidance.
- In `website/src/content/docs/docs/custom-software/publishing.mdx`, change the
  projected path to `/opt/agentos/pkgs/...` and correct the JS `pack --out`
  example: `--out` names the tar (for example
  `dist/my-agent-package.tar`), with `dist/my-agent-package.aospkg` emitted
  beside it. There is currently no checked public Markdown counterpart for
  this page.

### Exported SDK/Rust doc comments

After Item 50's type deletion, update only documentation/comments in:

- `packages/agentos-toolchain/src/manifest.ts`;
- `packages/manifest/src/index.ts`;
- `packages/core/src/agentos-package.ts`;
- `packages/core/src/packages.ts`;
- `packages/core/src/types.ts`;
- `packages/core/src/agent-os.ts`;
- `packages/agentos/src/actor.ts`;
- `crates/client/src/config.rs`;
- `crates/client/src/agent_os.rs`; and
- `crates/client/src/session.rs`.

Use “packed `.aospkg` path (transition directory only for local development),”
“embedded vbare manifest,” and `/opt/agentos/pkgs`. For `linkSoftware`, remove
the host-backed staging-directory and snapshot-persistence claims. Keep the
implementation as the same forwarding-only request.

## Extend the Item 38 claim verifier

Item 38 owns `scripts/verify-thin-client-docs.mjs`, its unit test, and CI wiring;
all three are present in the current ancestor stack. Item 51 should extend
those files, not create a second verifier or another CI invocation.

### Additional inputs

Keep Item 38's README and website roots, then add a fixed, reviewable guidance
inventory:

- `CLAUDE.md`, `crates/CLAUDE.md`, `crates/execution/CLAUDE.md`, and
  `packages/core/CLAUDE.md`;
- `examples/core/README.md` and the three checked custom-software snippets;
- `packages/agentos-toolchain/README.md` plus its CLI help source;
- `registry/README.md` and `registry/CONTRIBUTING.md`; and
- the ten toolchain/manifest/SDK/actor/Rust doc-comment files listed above.

Item 38's recursive website roots already include the limits, resource-limits,
and software source/public pairs discovered above. Add those exact paths to the
required-claim table so a future deletion is a failure rather than merely the
absence of a forbidden phrase.

The current verifier's exact extension seams are `guidanceRoots`,
`forbiddenClaims`, `requiredClaims`, and the `files` assembly inside
`auditThinClientDocs`. Add a `guidanceFiles` array for the fixed non-website
inventory above (including `README.md` and root `CLAUDE.md`), append each file to
`files`, and report `required-guidance-file` if one disappears. Keep the two
website roots recursive. In `scripts/verify-thin-client-docs.test.mjs`, change
`requiredContent` to use complete repository-relative keys and simplify
`writeValidFixture` to write those keys directly; otherwise every new CLAUDE,
example, registry, and source-comment input would be incorrectly created under
`website/public/docs/docs` by the current suffix-based helper.

Do not recursively scan tests, generated build output, the migration tracker,
or `docs/thin-client-research`; they intentionally quote obsolete claims as
fixtures/evidence. Keep paths slash-normalized and diagnostics sorted.

### New forbidden rules

Use path-scoped rules, not a global word ban:

| Rule ID | Reject | Important allowed case |
|---|---|---|
| `runtime-in-process` | “runs inside/in your process,” “boots ... in-process,” “in-process operating system kernel” in product/runtime pages | “sidecar-local/in-process limit registry” |
| `runtime-no-boundary` | “no client/server split” in core SDK guidance | “no actor runtime required” |
| `package-runtime-json` | claims that the sidecar reads projected `agentos-package.json` or that packed packages ship it | JSON described as toolchain/source input |
| `package-directory-primary` | “a package is a directory” as the normal runtime format | explicit “local transition directory” |
| `package-old-root` | `/opt/agentos/<name>` or `/opt/agentos/<pkg>` package roots | `/opt/agentos/bin` and `/opt/agentos/pkgs/...` |
| `package-dir-public-api` | `{ packageDir }` / `packageDir:` in public SDK/registry guidance | internal toolchain variable/CLI positional name |
| `deleted-software-cli` | `agentos-software link` | `linkSoftware` / `link_software` |
| `stale-v8-recovery` | “CURRENTLY BROKEN,” host-Node current-state claims, external recovery checkout | historical wording in this research note (outside inputs) |
| `deleted-crate-path` | backticked `crates/sidecar/...`, `crates/sidecar-browser/...`, `packages/secure-exec-core` in selected guidance | explicit negative note in `crates/native-sidecar/CLAUDE.md`, which is outside this rule's selected files |
| `legacy-product-owner` | secure-exec described as current runtime, kernel, sidecar, logger, registry, or repository owner in selected active guidance | root guidance explaining that secure-exec is a generated compatibility mirror; existing compatibility artifact names; `/__secure_exec` wire/guest paths |

Retain Item 38's path-scoped permission rules. Do not ban `deny-by-default`
globally because generic-kernel guidance is intentionally deny-all.

### Required positive claims

Add assertions that deletion cannot make the gate pass:

- root `CLAUDE.md`: clients are thin transports, omitted fields stay omitted,
  sidecar owns runtime/package/session/default behavior, and the TypeScript
  package-manager list is the sole exception;
- package architecture source and public copy: `.aospkg`, vbare runtime
  manifest, `/opt/agentos/pkgs`, `/opt/agentos/bin`, JSON stripped, and host
  `linkSoftware`;
- custom-software definition source/public: `{ packagePath }`, `.aospkg`, JSON
  as toolchain input, and the local-directory caveat;
- README/core page: shared sidecar process;
- registry/toolchain docs: `.aospkg` runtime output and `{ packagePath }`;
- limits/observability and resource-limits source/public pairs: AgentOS native
  sidecar ownership, `agentos_bridge::queue_tracker`, stderr, and `AGENTOS_LOG`;
- software source/public pair: this AgentOS repository owns `registry/`; and
- the Item 38 permission pages: AgentOS omission is allow-all.

### New verifier tests

Add focused Node test fixtures for:

1. each of the four original stale claim classes;
2. a deleted crate path in a selected CLAUDE file;
3. a missing required thin-client paragraph in root `CLAUDE.md`;
4. a legitimate `agentos-package.json` toolchain-input statement;
5. a legitimate local transition-directory statement;
6. a legitimate sidecar-local “in-process limit registry” statement;
7. a stale “secure-exec owns the runtime/registry” statement plus a legitimate
   root statement that secure-exec is only a generated compatibility mirror; and
8. the checked public Markdown copies, not only MDX sources.

The fixture helper should start from a minimal valid guidance tree and mutate
one claim at a time. This keeps the before test reproducible after the live docs
are corrected.

## Before and after validation

### Before evidence

After adding the new rules/tests but before editing guidance, run:

```bash
node scripts/verify-thin-client-docs.mjs
```

The live-repository audit must exit 1 and report at least:

- `README.md` for the in-process claim;
- `crates/CLAUDE.md` for broken V8/deleted crate paths and the permission
  contradiction;
- the package architecture page for runtime JSON, old projection root, and
  `agentos-software link`;
- the public custom-software copy for `packageDir`/runtime JSON;
- the limits/observability page for a legacy runtime/logger owner; and
- the software page for the obsolete secure-exec registry repository.

That is the Item 51 “test validated behavior before” checkbox. Capture the
command and representative rule IDs in the tracker evidence cell; do not commit
an intentionally failing test. **Do not expect the full Node test file to pass
at this intermediate point:** it already contains `passes on the current tree`,
so the newly added rules correctly make that test fail until the guidance is
fixed. The isolated mutation fixtures can be run by name while iterating, but
the full suite belongs in after validation.

### After evidence

Run:

```bash
node --test scripts/verify-thin-client-docs.test.mjs
node scripts/verify-thin-client-docs.mjs
node --check scripts/verify-thin-client-docs.mjs
pnpm --dir website build
pnpm --dir packages/agentos-toolchain test
pnpm --dir packages/agentos-toolchain check-types
pnpm --dir packages/core check-types
cargo test -p agentos-vfs-core --test package_format
cargo test -p agentos-native-sidecar --test package_projection
cargo fmt --all -- --check
git diff --check
```

The first four are required for Item 51. The remaining commands are cheap
source-of-truth checks for the package claims and catch accidental edits to CLI
help/doc comments near executable code. No full runtime suite is required for a
docs-only behavior change.

Add the verifier test and live audit to `.github/workflows/ci.yml` and
`scripts/ci.sh` only if Item 38 has not already done so. Item 51 should not add a
duplicate CI invocation.

## Dependencies and sequencing

- **Item 38 is already a sealed parent.** It created
  `scripts/verify-thin-client-docs.mjs`, its fixture suite, and CI wiring, and it
  owns the permission-default page edits. Item 51 extends that implementation;
  it must not create a competing verifier or duplicate CI steps.
- **Item 50 must be a parent.** It removes raw-string/deprecated TypeScript
  package descriptors. Update comments against the resulting
  `{ packagePath }`-only public type instead of preserving obsolete overloads.
- Item 39 may have changed the README quickstart in between. Item 51 owns only
  the runtime/process prose at lines 16, 24, and 135; preserve the executable
  quickstart. Item 55 separately owns the hand-maintained README API/package
  inventory.
- This remains a documentation-truth revision. `packages/agentos-toolchain/src/cli.ts`
  changes only its help/result wording; no package projection, protocol,
  permission, or client runtime behavior belongs here.

## Bounded JJ revision path set

Create one child revision for Item 51 after Item 50 and keep it to these
guidance/verifier paths (Item 38/50 edits will already be in the parent):

```text
README.md
crates/CLAUDE.md
crates/execution/CLAUDE.md
packages/core/CLAUDE.md
packages/manifest/src/index.ts
packages/core/src/agentos-package.ts
packages/core/src/packages.ts
packages/core/src/types.ts
packages/core/src/agent-os.ts
packages/agentos/src/actor.ts
crates/client/src/config.rs
crates/client/src/agent_os.rs
crates/client/src/session.rs
packages/agentos-toolchain/README.md
packages/agentos-toolchain/src/cli.ts
packages/agentos-toolchain/src/build.ts
packages/agentos-toolchain/src/manifest.ts
packages/agentos-toolchain/src/pack.ts
registry/README.md
registry/CONTRIBUTING.md
examples/core/README.md
examples/software/quickstart-node/my-tool.ts
examples/software/quickstart-wasm/my-cmds.ts
examples/software/quickstart-agent/my-agent.ts
website/src/content/docs/docs/core.mdx
website/src/content/docs/docs/versus-sandbox.mdx
website/src/content/docs/docs/architecture/networking.mdx
website/src/content/docs/docs/architecture/limits-and-observability.mdx
website/src/content/docs/docs/architecture/packages-and-command-resolution.mdx
website/src/content/docs/docs/resource-limits.mdx
website/src/content/docs/docs/software.mdx
website/src/content/docs/docs/custom-software/definition.mdx
website/src/content/docs/docs/custom-software/building-wasm.mdx
website/src/content/docs/docs/custom-software/publishing.mdx
website/src/content/docs/docs/agents/custom.mdx
website/public/docs/docs/versus-sandbox.md
website/public/docs/docs/architecture/networking.md
website/public/docs/docs/architecture/limits-and-observability.md
website/public/docs/docs/architecture/packages-and-command-resolution.md
website/public/docs/docs/resource-limits.md
website/public/docs/docs/software.md
website/public/docs/docs/custom-software/definition.md
website/public/docs/docs/custom-software/building-wasm.md
website/public/docs/docs/agents/custom.md
scripts/verify-thin-client-docs.mjs
scripts/verify-thin-client-docs.test.mjs
.github/workflows/ci.yml        # only if Item 38 did not wire the audit
scripts/ci.sh                  # only if Item 38 did not wire the audit
docs/thin-client-migration.md  # evidence/status only, last
```

Do not modify package projection/runtime code, protocol schemas, generated
website build output, the README API inventory (Item 55), or permission pages
already owned by Item 38. If correcting a claim would require changing runtime
behavior rather than describing the code above, stop and create a separate
numbered implementation item.

## Risks and review points

- **Source/public drift:** `pnpm --dir website build` does not refresh the
  checked `website/public/docs` Markdown. Review each listed pair explicitly.
- **Over-broad verifier regexes:** `agentos-package.json`, “directory,”
  “in-process,” and deny-default language all have legitimate scoped uses. Use
  target paths and sentence-level patterns.
- **Item overlap:** README and versus-sandbox permission lines belong to Item
  38; package descriptor source types belong to Item 50. Item 51 edits adjacent
  architecture wording only after those revisions are parents.
- **Dynamic-link persistence:** do not retain the current snapshot-persistence
  promise. Package mounts are external sidecar configuration, not serialized
  filesystem-layer contents.
- **Transition directories:** do not claim they are rejected today. They remain
  a sidecar-owned local-development compatibility path, but must not be taught
  as the production artifact.
- **Browser parity:** the browser sidecar consumes the same package/protocol
  concepts. Avoid native-only language when describing the package format; use
  “shared sidecar/runtime,” and reserve “child process” for native SDK topology.
