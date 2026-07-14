# Item 66 research: forward the shell's selected packages unchanged

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 66 tracker status.

Inspected on **2026-07-14** at revision **`d9eedb7278b9`**. Tracker anchors are
`docs/thin-client-migration.md:112` (issue inventory), current line 193
(pending status), and current line 279 (before/after/complete checklist).

## Recommendation

Delete all package-path probing, manifest inspection, fallback substitution, and
package skipping from `packages/shell/src/main.ts`. Keep the shell's static
TypeScript package selection, move that data-only list to an importable helper,
and pass every selected descriptor to the existing `AgentOs.create` or actor
path unchanged.

Priority: **P1**. Confidence: **high**.

## Cross-layer disposition

| Layer | Exact current code | Item 66 disposition |
|---|---|---|
| Shell package-manager selection | `packages/shell/src/main.ts:16-63,90-184`, consumed by both launch paths at `:692-698` | **Change.** Move the exact 23-entry selection to `software.ts`; delete all probing, fallback substitution, warning, and skipping; pass the same array to direct and actor creation. |
| TypeScript core serializer | `normalizePackageRef` at `packages/core/src/agent-os.ts:672-690`, exact-path deduplication/forwarding at `:1368-1403` | **No change.** It already validates only shape and sends `{ path }` without touching the filesystem or manifest. |
| Actor serializer | `packages/agentos/src/actor.ts:103-149`, with structural coverage at `packages/agentos/tests/actor.test.ts:450-471` | **No change.** It performs the same shape-only path conversion and exact-string deduplication. |
| TypeScript runtime/protocol | `LivePackageDescriptor` and generated conversion in `packages/runtime-core/src/descriptors.ts:106-157`; initialize serialization in `packages/runtime-core/src/request-payloads.ts:297-317`; BARE union in `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:205-222,255-262` | **No change.** Native paths and browser-owned inline bytes remain opaque package inputs; no parsed metadata crosses the wire. |
| Native sidecar | wire conversion in `crates/native-sidecar/src/vm.rs:1875-1893`, configuration/projection at `:447-518`, manifest/path ownership in `crates/native-sidecar/src/package_projection.rs:189-270,349-425` | **Production unchanged; strengthen tests.** Missing/corrupt selected paths must reach this code and fail with their sidecar-owned context. |
| Browser sidecar | host paths rejected and inline bytes decoded at `crates/native-sidecar-browser/src/wire_dispatch.rs:760-799,816-838` | **No change.** The Node shell uses the native path form; browser package managers already supply full opaque bytes. |
| Rust SDK | `PackageRef` at `crates/client/src/config.rs:142-151` and path-only wire construction in `crates/client/src/agent_os.rs:1859-1871` | **No change.** Rust already forwards every typed path without filtering, fallback, or manifest reads. |
| Docs/development tooling | `packages/shell/CLAUDE.md:1-8` and `justfile:52-132` | **No change.** Toolchain preflight builds missing local artifacts; runtime clients must fail on bad selected refs instead of repairing a checkout. |

The only allowed client behavior here is TypeScript package-manager selection,
the explicit exception in the thin-client rule. Selection means choosing the
static package inputs. It does not authorize checking, rewriting, or dropping
the selected artifacts.

This is the intended boundary:

```text
TypeScript package-manager layer
  static list of 23 package descriptors
            |
            | unchanged paths, including missing/bad paths
            v
native/actor client serializer
            |
            v
sidecar package reader + semantic validator + projection owner
```

Do not move the shell's local native-command fallback into the sidecar. It is a
development workaround that substitutes one unrelated directory for many
distinct packages, not a legitimate runtime default. The existing `just shell`
recipe already owns development build preflight outside the client/runtime.

## Original issue

The tracker entries are at `docs/thin-client-migration.md:112,193,279`:

> `packages/shell/src/main.ts` performs host `existsSync`/`statSync`/manifest
> reads, replaces missing package refs with one local native command directory,
> and skips packages on failure.

This is observable client-side policy rather than serialization:

- a valid selected descriptor is reconstructed as a new `{ packagePath }`
  object;
- a missing or unreadable package is silently changed to a shared local command
  directory when one happens to exist;
- a missing or unreadable package is warned about and removed when the fallback
  does not exist; and
- `vix`/`vim` are removed based on client reads of `agentos-package.json` and
  `package.json`.

The resulting sidecar request does not describe what the package-manager layer
selected, so the sidecar cannot report the real bad package path. Two users
passing the same descriptors can also get different package sets based on the
client process's host filesystem and checkout layout.

## Exact current code and behavior

### Package probing and substitution in `packages/shell/src/main.ts`

The complete policy is concentrated near the top of the file:

- lines 16-27 import `existsSync`, `statSync`, path joining/directory helpers,
  and `fileURLToPath` for package inspection/fallback resolution;
- lines 56-63 derive `workspaceRoot` from the shell module and hard-code
  `registry/native/target/wasm32-wasip1/release/commands` as
  `fallbackCommandDirs`;
- lines 90-96 introduce the local, permissive `RegistryPackage` type;
- `isUsablePackageDir` at lines 98-101 requires a host-side
  `agentos-package.json`;
- `isUsablePackageFile` at lines 103-110 calls `statSync` and converts every
  failure into `false`; and
- `withLocalCommandFallback` at lines 112-134 replaces a bad selected path with
  the first usable fallback directory, otherwise warns and returns `null`.

The main static list at lines 136-160 contains 21 package descriptors and runs
them through `.map(withLocalCommandFallback).filter(...)`. It selects, in order:

```text
coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip, curl, zip,
unzip, jq, ripgrep, fd, tree, file, yq, codex, git, httpGet, sqlite3
```

The editor loop at lines 162-184 applies a separate policy to `[vix, vim]`:

1. forward a host path only if `statSync(...).isFile()`;
2. for a directory, require `agentos-package.json`;
3. read and parse `package.json` in the client;
4. include it only if `bin` is a non-empty object; and
5. silently omit unreadable, placeholder, or commandless packages.

Finally, lines 692-698 pass the altered `software` array to either
`createActorShellVm` or `AgentOs.create`, with `defaultSoftware: false`.
Therefore all fallback and omission decisions happen before either sidecar
route receives a request.

### The registry modules already provide serializable inputs

Each selected `@agentos-software/*` module exports the same kind of descriptor.
For example, `registry/software/coreutils/src/index.ts` computes the URL of its
packed `package.aospkg` and exports:

```ts
export default { packagePath } satisfies SoftwarePackageRef;
```

The corresponding modules for `codex-cli`, `vix`, `vim`, and the remaining
software do the same. The shell does not need to reinterpret those descriptors.
Selecting this default package list in TypeScript is the explicit package-manager
exception to the thin-client rule; validating the selected artifacts is not.

### The normal client paths already serialize package paths

No protocol or production sidecar change is required:

- `normalizePackageRef` in `packages/core/src/agent-os.ts:672-690` accepts a raw
  path or `{ packagePath }` and returns the path without reading it;
- `AgentOs.create` at `agent-os.ts:1368-1403` flattens the package-manager input,
  removes exact duplicate path strings, and sends `{ path: ref.path }` entries;
- `normalizePackageRef`/`normalizedPackageRefs` in
  `packages/agentos/src/actor.ts:103-128` perform the equivalent structural
  actor serialization; and
- `buildConfigJson` at `actor.ts:130-149` emits those paths in `packages`.

Keep these paths structural. Item 66 must not add `stat`, `realpath`, inode,
manifest, command, or package-content inspection to either normalizer as a new
way to identify packages. Exact path-string forwarding/deduplication is the
current bounded behavior.

### The sidecar is already the semantic owner

The native sidecar consumes the forwarded path at the correct enforcement
point:

- `package_descriptor_from_wire` in
  `crates/native-sidecar/src/vm.rs:1558-1575` sends every wire `PackagePath` to
  `read_package_manifest_from_path`;
- VM configuration at `vm.rs:463-507` resolves descriptors, validates their
  projected commands/agent bundle/provides, and builds package mounts;
- `read_package_manifest_from_path` in
  `crates/native-sidecar/src/package_projection.rs:349-388` rejects an empty
  path, reads a file as a packed `.aospkg`, or treats a directory as a transition
  package; and
- `read_package_manifest_from_dir` at `package_projection.rs:199-246` rejects a
  transition directory without `package.aospkg` or `agentos-package.json`, reads
  the manifest, derives commands/manpages, and returns typed sidecar errors.

Projection validation continues at `build_package_leaf_mounts` beginning at
`package_projection.rs:390`, including duplicate commands and invalid ACP
entrypoints. A nonexistent selected path naturally reaches
`read_package_manifest`, then produces the existing sidecar package-dir error.
That error is preferable to a shell substitution because it names the selected
path and stops VM creation.

## Exact production edits

### Add `packages/shell/src/software.ts`

Move the 23 package imports out of the executable `main.ts` and export one
data-only array. Preserve the current selection and exact order, including
`vix` before `vim`:

```ts
import codex from "@agentos-software/codex-cli";
import coreutils from "@agentos-software/coreutils";
// ...the remaining existing package imports...
import type { SoftwareInput } from "@rivet-dev/agentos-core";

export const shellSoftware = [
	coreutils,
	sed,
	grep,
	gawk,
	findutils,
	diffutils,
	tar,
	gzip,
	curl,
	zip,
	unzip,
	jq,
	ripgrep,
	fd,
	tree,
	file,
	yq,
	codex,
	git,
	httpGet,
	sqlite3,
	vix,
	vim,
] satisfies SoftwareInput[];
```

This module must contain no `node:fs`, `node:path`, or `node:url` import and no
function that filters, copies, resolves, canonicalizes, or validates a package
descriptor. A separate module is useful because importing `main.ts` executes the
CLI and creates a VM, while the data-only selection can be tested directly.

### Simplify `packages/shell/src/main.ts`

1. Remove all 23 `@agentos-software/*` imports and the `SoftwareInput` type
   import.
2. Import `shellSoftware` from `./software.js`.
3. Delete `__dirname`, `workspaceRoot`, and `fallbackCommandDirs`.
4. Delete `RegistryPackage`, `isUsablePackageDir`, `isUsablePackageFile`,
   `withLocalCommandFallback`, the mapped/filtered `software` declaration, and
   the entire editor loop.
5. Pass `software: shellSoftware` to both `createActorShellVm` and
   `AgentOs.create`, retaining `defaultSoftware: false`.
6. Reduce the Node imports to what the rest of this file actually uses:
   `readFileSync` from `node:fs` and `basename`/`resolve` from `node:path`.
   Remove `node:os` and `node:url`.

Do **not** remove `readFileSync(resolve(envFilePath), "utf8")` at approximately
line 344. `--env-file` is an explicit caller-supplied host input, so parsing it
is CLI input handling rather than package/runtime bootstrap. Likewise, retain
host-path normalization for explicit `--volume`/`--mount` values.

### Do not change production sidecar code

The sidecar already owns package existence, format, manifest, command,
entrypoint, and projection validation. Item 66 needs test coverage for that
existing behavior, not a fallback, package default, or alternate build-output
directory in Rust.

## Development-build behavior

The removed fallback was attempting to make an incompletely built checkout
start. That responsibility already has a better home:

- `justfile:52-86` scans the shell's linked package outputs and builds missing
  registry packages before launching;
- `justfile:87-96` builds the common/core/actor TypeScript outputs when needed;
- `justfile:115-132` builds and pins the in-repo native sidecar; and
- `packages/shell/CLAUDE.md:1-8` tells contributors that `just shell` repairs
  dependencies, rebuilds missing shell software, and that the shell loads every
  command-providing registry package.

Keep this development preflight in the recipe/toolchain. Directly invoking a
published or workspace shell with a missing package should fail through the
sidecar; it should not fabricate a different package set. No filesystem access
is needed in the shell startup path to bootstrap packages.

## Test plan

### Before-behavior evidence

The tracker explicitly asks for proof that the old client policy changes input
before a sidecar request. Capture that evidence before deleting the code:

1. Extract the current selection logic unchanged into a testable helper as an
   intermediate local step, or test a temporarily exported helper.
2. Stub `statSync`/`existsSync` so a selected descriptor is missing while the
   native fallback directory appears usable. Assert the returned entry contains
   the fallback path and not the selected path.
3. Stub both selected and fallback paths as missing. Assert the selected entry
   is absent and the warning fires.
4. Stub a `vix`/`vim` directory with an unreadable or empty `package.json`. Assert
   it is absent without creating `AgentOs` or invoking `createActorShellVm`.

Record the focused passing test command and vulnerable parent revision in the
Item 66 tracker evidence. These old-policy assertions should not remain in the
final revision; replace them with forwarding assertions.

Research-time baseline evidence at `d9eedb7278b9`:

| Check | Result |
|---|---|
| Source inventory of `packages/shell/src/main.ts:98-184` | **Vulnerable behavior present:** selected paths are statted, substituted with one local fallback, warned/skipped, or omitted after editor-manifest reads before either VM factory runs. |
| `cargo test -p agentos-native-sidecar --test package_projection` | **Pass: 11 passed.** Existing sidecar tests cover transition/packed projection, missing manifest, duplicate command, and invalid entrypoint behavior. |
| `pnpm --dir packages/core exec vitest run tests/options-schema.test.ts --fileParallelism=false` | **Pass: 12 passed.** Future/nonexistent-looking refs remain structurally valid. |
| `AGENTOS_SIDECAR_BIN=$PWD/target/debug/agentos-sidecar pnpm --dir packages/agentos exec vitest run tests/actor.test.ts --fileParallelism=false` | **Pass: 15 passed.** The environment variable is required in this checkout because the optional platform package is not installed. |
| `pnpm --dir packages/shell check-types` | **Environment-blocked before Item 66:** several selected registry packages and `@rivet-dev/agentos` declarations have not been built. Run the repository build prerequisite before using this as implementation evidence. |

### After: shell forwards the complete static selection

Add `packages/shell/tests/software.test.ts` against the data-only module. It
should prove:

- all 23 imported descriptors appear once, in the exact documented order;
- descriptor objects/paths are retained unchanged rather than reconstructed;
- descriptors whose strings look nonexistent, unreadable, or commandless are
  still present when package modules are mocked with such values;
- importing/selecting the list does not call `existsSync`, `statSync`,
  `readFileSync`, `realpathSync`, or emit a package-skip warning; and
- the selection source has no fallback directory or package-manifest probe.

Prefer module mocks plus identity/order assertions over duplicating package
validation in the test. A small source-boundary assertion for forbidden package
probe names is acceptable because absence of host filesystem policy is part of
the architecture contract.

Keep the existing `packages/shell/tests/cli.test.ts` smoke coverage. It tests
real shell commands, mounts, environment input, stdin, and direct/actor launch;
it should continue to use the fully built package registry from the test
preflight.

### After: sidecar reports bad refs and projects good refs

Extend `crates/native-sidecar/tests/package_projection.rs` with focused calls to
`read_package_manifest_from_path`:

- pass a nonexistent path and assert the sidecar returns an error containing
  that original path/package-dir context; and
- write a corrupt file with an `.aospkg` name, pass it unchanged, and assert the
  vbare/package-header error is returned.

Retain the existing tests:

- `reads_version_from_agentos_package_json_and_errors_when_missing` proves an
  empty transition directory is rejected;
- `reads_manifest_and_commands_from_package_tar_without_extracting` proves a
  good packed package is read;
- the tar and transition-dir mount tests prove both projection forms; and
- duplicate-command and invalid-agent-entrypoint tests prove semantic errors
  remain sidecar-owned.

The TypeScript and actor serializer coverage at
`packages/core/tests/options-schema.test.ts:57-84` and
`packages/agentos/tests/actor.test.ts:450-471` already proves nonexistent/future
paths are structurally accepted and emitted unchanged. Retain and run those
tests; do not relocate their purely structural assertions into the sidecar.

## Validation commands

Run the focused boundary checks first:

```bash
pnpm build
pnpm --dir packages/shell exec vitest run tests/software.test.ts --fileParallelism=false
pnpm --dir packages/shell check-types
cargo test -p agentos-native-sidecar --test package_projection
AGENTOS_SIDECAR_BIN="$PWD/target/debug/agentos-sidecar" \
  pnpm --dir packages/agentos exec vitest run tests/actor.test.ts --fileParallelism=false
pnpm --dir packages/core exec vitest run tests/options-schema.test.ts --fileParallelism=false
```

Then run the package-level and workspace gates in proportion to the final diff:

```bash
pnpm --dir packages/shell test
cargo check --workspace
git diff --check
```

`pnpm build` supplies the registry and actor declarations required by the
standalone shell type-check. The shell package test builds and launches the real
CLI, so it is the expensive check; the data-only selection test should diagnose
Item 66 failures first.

## Bounded JJ revision

Item 66 must be one dedicated stacked `jj` revision containing only:

```text
packages/shell/src/main.ts
packages/shell/src/software.ts
packages/shell/tests/software.test.ts
crates/native-sidecar/tests/package_projection.rs
docs/thin-client-migration.md
```

The Rust file is test-only. No production core, actor, protocol, sidecar,
registry package, lockfile, `justfile`, or shell `CLAUDE.md` edit should be
needed. Before describing/squashing the revision, inspect its paths explicitly
because this workspace contains extensive unrelated shared changes.

Recommended revision description:

```text
refactor(shell): forward selected packages unchanged
```

## Dependencies, overlaps, and risks

- **Item 27 dependency:** retain its structural package-path forwarding. Do not
  reintroduce client `stat`/`realpath`/inode or package-content deduplication.
- **Item 60 overlap:** it also changes `packages/shell/src/main.ts` around stdin
  queue handling. Stack Item 66 after it or preserve that hunk while applying
  the top-of-file/list cleanup.
- **Item 51 overlap:** its documentation audit may touch shell guidance. Item 66
  needs no guidance change because `packages/shell/CLAUDE.md` already assigns
  missing-package builds to `just shell` and requires the complete registry
  list.
- **Static-list drift:** the package list has 23 entries while the package
  manifest also declares currently unused `duckdb` and `wget` dependencies.
  Dependency cleanup belongs to Item 49/50 or another dedicated revision; do
  not silently add/remove packages under Item 66.
- **Intentional failure change:** a missing editor/package now fails VM startup
  through the sidecar instead of being omitted. This is the desired fail-closed,
  diagnosable behavior. The `just shell` preflight must keep normal repository
  development green.
- **Actor parity:** both direct and actor launch must receive the same
  `shellSoftware` array and `defaultSoftware: false`; testing only the direct
  call would leave a divergent path possible.
- **No client startup bootstrap:** importing descriptors is allowed package
  selection. Opening, statting, canonicalizing, unpacking, or fabricating their
  host files during client startup is not.

## Completion checklist

- [ ] Before-behavior test evidence records fallback substitution, omission,
  and editor skipping before sidecar construction.
- [ ] `main.ts` contains no package filesystem probe, manifest read, fallback
  directory, or package-skip branch.
- [ ] The data-only list forwards all 23 selected descriptors unchanged to both
  direct and actor paths.
- [ ] Sidecar tests reject the original missing/corrupt paths and retain valid
  packed/transition projection coverage.
- [ ] Focused TypeScript, Rust, actor, and serializer validations pass.
- [ ] The dedicated Item 66 `jj` revision contains only the bounded paths above.
- [ ] `docs/thin-client-migration.md` records before evidence, after evidence,
  revision ID, and marks Item 66 `done` only after all checks pass.
