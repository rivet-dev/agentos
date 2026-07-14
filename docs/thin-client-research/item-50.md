# Item 50 research: remove the TypeScript string package descriptor

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 50 tracker status.

Inspected: **2026-07-14**, shared working copy `8e163651c62b`. Symbol names and
code shapes below are the stable anchors if earlier stacked revisions move the
numeric line positions.

## Recommendation

Remove the core SDK's deprecated string package surface completely:

- delete the public `PackageRef` and `PackageDescriptor` string aliases;
- delete `isPackageDescriptor`, whose only behavior is `typeof value ===
  "string"`;
- accept only `{ packagePath: string }` in `AgentOsOptions.software`, meta-package
  arrays, actor options, `defineSoftware`, and `AgentOs.linkSoftware`;
- retain `defineSoftware` as a typed identity helper for the supported object;
  and
- add a focused TypeScript configuration that compiles
  `tests/public-api-exports.test.ts` as part of the core package's normal
  `check-types` gate.

The client still forwards only the `packagePath` string over the sidecar
protocol. This change removes an alternate JavaScript SDK descriptor shape; it
does not add package parsing, existence checks, or projection behavior to the
client.

Priority: **P2**. Confidence: **high**. Repository-wide usage shows that the
legacy aliases and discriminator are used only by their own tests, while all
registry packages and checked examples already use `{ packagePath }`. The
remaining raw-string uses are local test fixtures and three actor fixtures that
can be converted mechanically.

## Original issue and before evidence

The tracker entry is at `docs/thin-client-migration.md:96,183,275`.

### The deprecated public type is literally a string

`packages/core/src/agentos-package.ts:15-35` imports the migration-era manifest
`PackageRef = string`, re-exports it, aliases it as a deprecated
`PackageDescriptor`, and provides a public discriminator:

```ts
export type PackageRef = ManifestPackageRef;
export type SoftwarePackageRef = { packagePath: string };
/** @deprecated Package software is now represented by its package directory. */
export type PackageDescriptor = PackageRef;

export function isPackageDescriptor(
	value: unknown,
): value is PackageDescriptor {
	return typeof value === "string";
}
```

The aliases are re-exported from `packages/core/src/types.ts:90-95`, and the
guard is re-exported from `packages/core/src/index.ts:33-37`. No production code
calls the guard. Its only consumers are:

- `packages/core/tests/agentos-package.test.ts`, which asserts strings are the
  legacy descriptor; and
- `packages/core/tests/public-api-exports.test.ts:17,76`, which asserts the
  guard remains public.

The manifest package's own unrelated `PackageDescriptor` interface describes
package authoring metadata, and the sidecar protocol's `PackageDescriptor`
union describes path/inline transport. Do not delete either merely because the
names overlap.

### Runtime paths still accept the removed shape

The core source type `SoftwareInput` at `packages/core/src/packages.ts:10-11`
already contains only `SoftwarePackageRef` objects and their one-level arrays.
Despite that, three runtime surfaces retain string compatibility:

- `packages/core/src/options-schema.ts:268-275` unions `z.string()` with the
  object schema;
- `packages/core/src/agent-os.ts:671-685` calls a raw string “shorthand” and
  normalizes it to a path;
- `AgentOs.linkSoftware` at `agent-os.ts:2135-2138` explicitly accepts
  `PackageRef | SoftwarePackageRef`.

The actor copies the same branch at
`packages/agentos/src/actor.ts:103-116`, and its native options schema consumes
the shared core software schema. This is duplicated client compatibility, not a
sidecar requirement.

### The public API test is transpiled but not typechecked

`packages/core/tests/public-api-exports.test.ts:75-77` currently says:

```ts
expect(defineSoftware("/opt/pkg")).toBe("/opt/pkg");
expect(isPackageDescriptor).toBeTypeOf("function");
```

`defineSoftware` is correctly declared as
`T extends SoftwarePackageRef` at `packages/core/src/packages.ts:20-22`, so the
string call is already unsupported by the source type. Vitest transpiles the
test without checking it, and `packages/core/tsconfig.json` includes only
`src/**/*`. Consequently the runtime identity function returns the invalid
string and the test passes.

The current baseline demonstrates the gap exactly. The combined runtime
characterization passes all 19 tests across the public export, legacy guard, and
software-schema files:

```console
$ pnpm --dir packages/core exec vitest run \
    tests/public-api-exports.test.ts \
    tests/agentos-package.test.ts \
    tests/options-schema.test.ts
Test Files  3 passed (3)
Tests       19 passed (19)

$ pnpm exec tsc --noEmit --target esnext --module NodeNext \
    --moduleResolution NodeNext --strict --skipLibCheck --types node \
    packages/core/tests/public-api-exports.test.ts
public-api-exports.test.ts(76,25): error TS2345: Argument of type 'string' is
not assignable to parameter of type 'SoftwarePackageRef'.
```

That focused compiler invocation also reports three existing `TS18048`
diagnostics where the test dereferences optional mount plugin config at lines
110, 111, and 117. Reshape those assertions as part of enabling the gate; do not
weaken compiler settings.

The actor's configuration-only characterization is also independently runnable
without booting RivetKit or resolving a native sidecar binary:

```console
$ pnpm --dir packages/agentos exec vitest run tests/actor.test.ts \
    -t 'buildConfigJson|auto-injects|does not duplicate|explicit /root|rejects removed|rejects an unrecognized'
Test Files  1 passed (1)
Tests       7 passed | 8 skipped (15)
```

Running the whole actor file in this checkout reaches four unrelated integration
tests and fails because the optional published platform sidecar package is not
installed. Use the filtered command for this item's fast before/after gate; use
the full file only with `AGENTOS_PLUGIN_BIN` and `AGENTOS_SIDECAR_BIN` pointing
at built checkout binaries.

### Documentation and checked examples

All checked `defineSoftware` consumers already use the supported object form:

- `examples/custom/pi-cli.ts`;
- `examples/software/quickstart-{node,wasm,agent}/*.ts`; and
- `website/src/content/docs/docs/{agents/custom,custom-software/definition}.mdx`.

They need no Item 50 behavior edit and serve as positive compile examples. Two
known documentation surfaces are stale for broader reasons:

- `packages/agentos-toolchain/README.md` still shows `{ name, dir }`; and
- generated `website/public/docs/**` still contains old `{ packageDir }` output.

Item 51 already owns those source/generated documentation corrections. Do not
hand-edit generated website output or expand Item 50 into package-format prose.
Within Item 50, update the legacy `software: [<dir>]` comments in touched fixture
files, plus `packages/core/tests/pty-line-discipline.test.ts:121-124`, to show
`{ packagePath }` even though that PTY test already uses the correct object.

`packages/core/tests/options-schema.test.ts:66-85` is the separate runtime
“before” test: it currently lists a raw string among accepted serializable
software refs. `packages/core/tests/agentos-package.test.ts:9-15` is the legacy
export “before” test.

## Exact core production edits

### `packages/core/src/agentos-package.ts`

Remove `PackageRef as ManifestPackageRef` from the manifest import. Delete:

- `PackageRef`;
- deprecated `PackageDescriptor`; and
- `isPackageDescriptor`.

Keep `AgentBlock`, `SoftwarePackageRef`, `OPT_AGENTOS_ROOT`, and
`OPT_AGENTOS_BIN`. The resulting client descriptor surface is only:

```ts
export type SoftwarePackageRef = { packagePath: string };
```

Do not rename this to the protocol's `PackageDescriptor`; keeping the client
input and wire transport concepts distinct avoids restoring the old ambiguity.

### `packages/core/src/index.ts` and `packages/core/src/types.ts`

In `index.ts:33-37`, remove `isPackageDescriptor` and keep only the two path
constants in that value export block.

In `types.ts:90-95`, remove `PackageDescriptor` and `PackageRef`; keep
`AgentBlock` and `SoftwarePackageRef`.

### `packages/core/src/options-schema.ts`

Replace the union at lines 268-271:

```ts
const softwarePackageRefSchema = z.object({ packagePath: z.string() });
```

Keep `softwareInputSchema` as the union of one object and an array of those
objects. Meta-packages remain supported. Zod may accept/strip future object
fields as it does today; the client needs only `packagePath` and must not inspect
package semantics.

### `packages/core/src/agent-os.ts`

Remove `PackageRef` from the import at lines 107-108.

Delete the string branch from `normalizePackageRef` at lines 671-685. Keep the
structural runtime check because untyped JavaScript can still call the SDK:

```ts
function normalizePackageRef(value: unknown): NormalizedPackageRef {
	const record = toRecord(value);
	if (typeof record.packagePath === "string") {
		return { path: record.packagePath };
	}
	throw new TypeError(
		"Invalid software package reference: expected { packagePath: string }",
	);
}
```

Do not read the path or inspect an `.aospkg`; `normalizePackageRef` remains only
structural serialization.

Narrow `linkSoftware` at lines 2135-2138 to:

```ts
async linkSoftware(descriptor: SoftwarePackageRef): Promise<void>
```

It should still call the same normalizer and forward `{ path: ref.path }` to
`linkPackage`. This ensures an untyped string also receives the same explicit
runtime error rather than becoming `{ path: undefined }`.

No change is needed in `packages/core/src/packages.ts`: `SoftwareEntry`,
`SoftwareInput`, and `defineSoftware` already describe the correct object shape.
Update its package-dir wording only if Item 51 has not already corrected it; do
not broaden Item 50 into package-format documentation cleanup.

## Exact actor edit

In `packages/agentos/src/actor.ts:103-116`, delete the raw-string branch and
change the error to:

```text
Invalid software package reference: expected { packagePath: string }
```

Retain the object check, exact-path deduplication, meta-package flattening, and
conversion to `{ packagePath: ref.path }` for the Rust actor plugin. No Rust
change is needed: `crates/agentos-actor-plugin/src/config.rs` already receives
the canonical `packagePath` object form.

This actor edit is required even though the shared Zod schema rejects strings.
It removes the copied compatibility behavior and keeps the serializer total if
it is ever called with unchecked input.

## Make the public API test a real type gate

### Add `packages/core/tests/tsconfig.public-api.json`

Use a focused test config rather than adding the entire runtime/e2e suite to the
library declaration build:

```json
{
	"extends": "../tsconfig.json",
	"compilerOptions": {
		"noEmit": true,
		"rootDir": ".."
	},
	"include": ["./public-api-exports.test.ts"]
}
```

Overriding `rootDir` is necessary because the inherited core config points at
`src/`. Do not disable `strict`, add `any`, or use a transpile-only checker.

### `packages/core/package.json`

Append the focused gate to the existing scoped script:

```json
"check-types": "pnpm run build:protocols && tsc --noEmit && tsc -p tests/tsconfig.public-api.json"
```

The root remains Turbo orchestration only, as required by the repository
working agreement.

### `packages/core/tests/public-api-exports.test.ts`

Make these changes:

1. Import the root module as a namespace for a runtime absence assertion.
2. Delete the named `isPackageDescriptor` import and expectation.
3. Replace the string identity assertion with an object assertion:

   ```ts
   const software = defineSoftware({ packagePath: "/opt/pkg" });
   expect(software).toEqual({ packagePath: "/opt/pkg" });
   ```

4. Add a compile-only negative case in an unreachable branch:

   ```ts
   if (false) {
     // @ts-expect-error raw string package descriptors are not supported
     defineSoftware("/opt/pkg");
   }
   ```

   The new test tsconfig makes the directive fail as unused if string support is
   accidentally restored.
5. Assert `"isPackageDescriptor" in rootApi` is false. Add compile-time absence
   assertions for `PackageDescriptor` and `PackageRef`, for example with
   `@ts-expect-error` type queries against `import("../src/index.js")`. This
   proves both value and type legacy exports are gone.
6. Replace direct optional `mount.plugin.config.hostPath/readOnly` property
   access at lines 109-116 with `toMatchObject`/whole-object assertions so the
   focused strict compile passes without non-null assertions.

Keep `SoftwarePackageRef` exported and add it to the public type smoke section
if it is not already covered. The desired contract is removal of the string
aliases, not removal of the canonical descriptor type.

## Runtime fixture migration

Delete `packages/core/tests/agentos-package.test.ts`; after the guard is removed
it has no remaining behavior to test. The root API test owns the absence check.

Update every local string package fixture to wrap the path without changing the
package contents or sidecar assertions:

```ts
software: [{ packagePath: pkgDir }]
await vm.linkSoftware({ packagePath: pkgDir })
```

The current raw-string call sites are:

- `packages/core/tests/agentos-projection-isolation.test.ts:45`;
- `packages/core/tests/agentos-package-agent-vm.test.ts:104`;
- `packages/core/tests/agentos-package-vm.test.ts:47`;
- `packages/core/tests/agentos-package-real-agent.e2e.test.ts:75`;
- `packages/core/tests/list-agents.test.ts:45` (wrap both joined paths);
- `packages/core/tests/fs-native-parity.test.ts:188`;
- `packages/core/tests/pty-protocol.test.ts:183`; and
- all three calls in
  `packages/core/tests/agentos-package-link-vm.test.ts:65,94,119`.

The unchecked package-local snapshot driver
`packages/core/vim-snap.mjs:19-25,69-72` also returns a raw package-directory
string and passes it as a software leaf. Wrap its result at the call site (or
change `materializeVimPackage` to return `{ packagePath }`) so removing the
runtime compatibility branch does not leave a broken repository script. This
file is not part of the TypeScript compiler gate, which is why a source-only
audit is required in addition to `check-types`.

`packages/core/tests/pty-line-discipline.test.ts:706` already wraps its returned
string and needs no edit. Registry package imports such as `common`, `pi`, and
`coreutils` already default-export `{ packagePath }` and also need no edit.

In `packages/agentos/tests/actor.test.ts`, replace all three raw-string fixtures
at lines 456, 477, and 491 with `{ packagePath: "..." }`, preserving the expected
serialized objects. Add a rejected raw-string case beside the existing malformed
and removed-field cases at lines 529-550.

In `packages/core/tests/options-schema.test.ts`:

- move a valid-looking raw string into the rejected table at lines 57-64;
- remove the string from the accepted list at lines 74-83;
- retain one direct object, one extensible object, and one object meta-package
  array.

Add a `linkSoftware` runtime rejection test using a deliberately untyped value
(`as never` is acceptable only at this negative boundary). It should assert the
client rejects the string with the canonical `{ packagePath: string }` message
before any sidecar package request. Existing link tests then prove the object
form still performs live command/agent projection, idempotency, and duplicate
command rejection.

## Before and after test checklist

### Before behavior

- [ ] Record that `public-api-exports.test.ts` passes 6/6 under Vitest while the
  focused `tsc` command fails with `TS2345` on `defineSoftware(string)`.
- [ ] Record `agentos-package.test.ts` proving the legacy string discriminator
  is exported and returns true for strings.
- [ ] Record `options-schema.test.ts` and the actor config test accepting a raw
  string software entry.
- [ ] Record the real `agentos-package-link-vm.test.ts` object-independent
  functionality before changing its input spelling.

### After behavior

- [ ] `pnpm --dir packages/core check-types` compiles core source and the actual
  `public-api-exports.test.ts`, including the positive object and negative
  string assertions.
- [ ] `public-api-exports.test.ts` passes at runtime and proves the legacy value
  export is absent while `defineSoftware({ packagePath })` preserves its input.
- [ ] `options-schema.test.ts` accepts object/meta-object inputs and rejects a
  raw string plus all previously malformed entries.
- [ ] `packages/agentos/tests/actor.test.ts` proves the actor accepts/serializes
  the object form and rejects the string form.
- [ ] `agentos-package-link-vm.test.ts` proves object-form dynamic linking keeps
  live command/agent discovery, idempotency, and duplicate-command failures and
  rejects an untyped string before forwarding.
- [ ] Local fixture suites listed above still pass with object wrappers, showing
  the sidecar receives the same path and no package behavior moved client-side.

Focused validation commands:

```sh
pnpm --dir packages/core check-types
pnpm --dir packages/core exec vitest run \
  tests/public-api-exports.test.ts \
  tests/options-schema.test.ts \
  tests/agentos-package-link-vm.test.ts \
  tests/agentos-projection-isolation.test.ts \
  tests/agentos-package-agent-vm.test.ts \
  tests/agentos-package-vm.test.ts \
  tests/list-agents.test.ts
pnpm --dir packages/agentos check-types
pnpm --dir packages/agentos exec vitest run tests/actor.test.ts \
  -t 'buildConfigJson|auto-injects|does not duplicate|explicit /root|rejects removed|rejects an unrecognized'
pnpm --dir packages/shell check-types
pnpm check-types
```

Run the real-agent, PTY, and native filesystem fixture tests in the explicit
expensive validation phase according to their existing skip/build requirements.

## Risks and guards

- **Deleting the wrong descriptor:** do not touch the BARE
  `PackageDescriptor` path/inline union, runtime-core `LivePackageDescriptor`,
  native package projection descriptor, Rust client `PackageRef`, or the
  manifest's package-authoring `PackageDescriptor` interface.
- **Removing `defineSoftware`:** retain it. Checked examples and website source
  use it correctly with `{ packagePath }`; it provides inference for extensible
  descriptor objects without doing runtime policy work.
- **Moving package semantics into the client:** the object contains only a path.
  Existence, packed format, manifest, commands, agents, and projection remain
  sidecar-owned.
- **Leaving a hidden string path:** remove the schema branch, both normalizer
  branches, the `linkSoftware` union, and the core public aliases together. A
  type-only deletion while runtime schemas still accept strings is incomplete.
- **Breaking meta-packages:** keep the one-level `SoftwareInput[]`/flattening
  shape; only each leaf changes from “string or object” to object.
- **Weakening type checking:** fix the three optional-config assertions instead
  of adding `!`, `as any`, or disabling strictness. Keep the negative
  `@ts-expect-error` under the new focused gate.
- **Editing stale generated docs:** current website source already uses
  `{ packagePath }`; obsolete generated/public `packageDir` text and toolchain
  prose belong to Item 51, not this revision.
- **Reintroducing client filesystem work:** wrapping test paths in
  `{ packagePath }` is serialization only. Do not stat, resolve, pack, or parse
  those paths in either client.

## Dependencies and sequencing

- Item 27 established the canonical serializable `{ packagePath }` leaf and
  total object/meta-package normalization. Item 50 finishes that work by
  deleting the deliberately retained raw-string compatibility surface; do not
  restore any manifest parsing or path inspection.
- Stack Item 50 after Item 49 in its own revision. There is no semantic
  dependency on Item 49's dependency cleanup, but both touch
  `packages/core/package.json`, so rebase the focused `check-types` script edit
  rather than combining revisions.
- Item 51 owns stale architecture/package-format prose. Only correct comments
  directly made false by this removal; do not pull the website's broader
  package documentation migration into Item 50.
- No Rust, sidecar, protocol, or generated compatibility-mirror prerequisite is
  required. The supported wire payload remains the same path string.

## Ordered edit sequence

1. Record the three passing core characterization files, the focused `TS2345`
   failure, and the seven passing actor config tests in the tracker before cell.
2. Delete core's `PackageRef`, deprecated `PackageDescriptor`, and
   `isPackageDescriptor`, then remove only their core root/type re-exports.
3. Remove `z.string()` from the software schema; remove the raw-string branch
   from the core and actor normalizers; narrow `linkSoftware` to
   `SoftwarePackageRef`. Keep `{ packagePath } -> { path }` forwarding unchanged.
4. Convert every raw-string fixture enumerated above, add explicit schema/actor/
   `linkSoftware` rejection coverage, and correct adjacent legacy comments.
5. Convert the public API identity assertion to `{ packagePath }`, add compile-
   time negative/absence assertions, fix its strict optional-config assertions,
   and delete the now-empty guard-only test file.
6. Add the focused test tsconfig and append it to the scoped core `check-types`
   script. Run the focused runtime/type gates, then the root typecheck.
7. Mark Item 50's after and completion cells only after the dedicated revision
   ID and exact passing commands are recorded.

## Bounded dedicated JJ revision

Implement Item 50 in one new stacked `jj` revision, separate from every other
tracker item. The expected path set is bounded to:

```text
packages/core/src/agentos-package.ts
packages/core/src/agent-os.ts
packages/core/src/options-schema.ts
packages/core/src/index.ts
packages/core/src/types.ts
packages/core/package.json
packages/core/tests/tsconfig.public-api.json
packages/core/tests/public-api-exports.test.ts
packages/core/tests/agentos-package.test.ts                 # delete
packages/core/tests/options-schema.test.ts
packages/core/tests/agentos-package-link-vm.test.ts
packages/core/tests/agentos-projection-isolation.test.ts
packages/core/tests/agentos-package-agent-vm.test.ts
packages/core/tests/agentos-package-vm.test.ts
packages/core/tests/agentos-package-real-agent.e2e.test.ts
packages/core/tests/list-agents.test.ts
packages/core/tests/fs-native-parity.test.ts
packages/core/tests/pty-protocol.test.ts
packages/core/tests/pty-line-discipline.test.ts             # comment only
packages/core/vim-snap.mjs
packages/agentos/src/actor.ts
packages/agentos/tests/actor.test.ts
docs/thin-client-migration.md
```

No Rust, sidecar protocol, lockfile, dependency, registry package, generated
website, or manifest-package edit is expected. If an implementation compiler
audit finds another raw local path passed as a software leaf, add only that
fixture file to the same revision and document it in the tracker validation
cell. Mark Item 50 complete only after the before mismatch is recorded, the
focused public API type gate is part of `check-types`, the runtime compatibility
branches are gone, and the dedicated revision ID is written into the tracking
row.
