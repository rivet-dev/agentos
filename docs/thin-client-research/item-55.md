# Item 55 research: remove the stale Core README API inventory

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 55 tracker status.

Inspected at JJ revision `vsqvzlkntopu` (`ec1b22d69827`,
`fix(acp): reject malformed response envelopes`). Recheck line numbers after
earlier stacked items rebase this work.

Priority: **P3**. Confidence: **high**.

## Recommendation

Delete the hand-maintained method and type inventory from
`packages/core/README.md`. Replace it with a short documentation section that
links to the AgentOS guides and says the emitted `dist/index.d.ts` declarations
are the authoritative public API.

Extend the existing `scripts/verify-thin-client-docs.mjs` gate to audit the Core
README, reject a hand-maintained API inventory there, and require the
declaration-source statement. This is the smallest durable fix:

- Core already emits and publishes TypeScript declarations from its actual root
  exports.
- The current inventory is both incorrect and materially incomplete.
- Website TypeDoc targets `packages/agentos`, not `packages/core`.
- Generating Markdown into this README would add a new generator, generated
  artifact, and review surface without making the installed `.d.ts` files more
  authoritative.

Do not repair the tables row by row, generate a replacement table, change
runtime code to make the prose true, or add sidecar/Rust tests. Item 55 is a
documentation-source-of-truth defect, not client behavior.

## Original issue and current drift

`packages/core/README.md:55-204` copies 50 method signatures and 36 type names
by hand. Nothing derives this copy from `packages/core/src/index.ts`,
`packages/core/src/types.ts`, or emitted declarations.

The tracker names three concrete mismatches; all remain present:

| README claim | Authoritative declaration | Exact problem |
|---|---|---|
| `AgentOsOptions` contains `commandDirs` (`packages/core/README.md:152`) | `packages/core/src/agent-os.ts:454-529` has no such field; `packages/core/src/options-schema.ts:288-328` does not accept it | A removed option is documented. |
| `AgentConfig` is exported (`packages/core/README.md:184`) | `packages/core/src/index.ts:3-53` and `packages/core/src/types.ts:1-120` export no such type; no source declaration exists | The documented type does not exist. |
| `AgentRegistryEntry` has `acpAdapter` and `agentPackage` (`packages/core/README.md:185`) | `packages/core/src/agent-os.ts:95-100` declares only `id`, `installed`, and `adapterEntrypoint` | Two obsolete fields are listed and the current field is missing. |

The inventory has additional exact drift:

| README location | Current declaration mismatch |
|---|---|
| `:55` and `:149` | `## API Reference` and `### Exported Types` present a copied inventory as authoritative although it is not generated. |
| `:80` | `mkdir(path)` omits the supported `{ recursive?: boolean }` argument at `agent-os.ts:1818-1820`. |
| `:87-88` | `mountFs` and `unmountFs` claim `void`; both return `Promise<void>` at `agent-os.ts:1869-1878`. |
| `:95` | `spawn` names nonexistent `SpawnOptions`; the method accepts exported `KernelSpawnOptions` at `agent-os.ts:1706-1710`. |
| `:130` | `listAgents` omits `Promise`; the method returns `Promise<AgentRegistryEntry[]>` at `agent-os.ts:2168-2181`. |
| `:163-166` | `MountConfigMemory`, `MountConfigCustom`, and `MountConfigOverlay` do not exist. The current exported types are `PlainMountConfig`, `NativeMountConfig`, and `OverlayMountConfig` (`agent-os.ts:247-278`; `types.ts:23-33`). |
| `:167` | `chunkedS3MountPlugin()` is a value exported by `@rivet-dev/agentos-runtime-core/descriptors`, not by Core's root entrypoint, yet appears under Core's “Exported Types.” |
| `:183` | The `AgentType` bullet repeats the obsolete projected-JSON-manifest model. Item 51 owns correcting declaration comments and architecture guidance; deleting the duplicate bullet avoids overlap. |
| `:193-194` | `AgentCapabilities` is not only Boolean flags (`promptCapabilities` is structured and extension keys are allowed), and `AgentInfo` also includes `title`. |

The method list is also incomplete. It omits these current public methods:

- `execArgv` (`agent-os.ts:1609-1621`);
- `writeProcessStdin`, `closeProcessStdin`, `onProcessStdout`,
  `onProcessStderr`, `onProcessExit`, and `waitProcess`
  (`agent-os.ts:1738-1807`);
- `snapshotRootFilesystem` (`agent-os.ts:1852-1861`);
- `waitShell` (`agent-os.ts:1993-2009`);
- `linkSoftware` and `providedCommands` (`agent-os.ts:2135-2160`);
- `resumeSession` (`agent-os.ts:2737-2773`);
- `getSessionCapabilities`, `getSessionAgentInfo`, and `rawSessionSend`
  (`agent-os.ts:3028-3047`); and
- `scheduleCron`, `listCronJobs`, `cancelCronJob`, and `onCronEvent`
  (`agent-os.ts:3076-3092`).

A corrected table would therefore be a large new snapshot of a moving API, not
a durable repair.

## Existing source of truth

No new API generator is needed:

- `packages/core/src/index.ts` defines the root value/type export surface.
- `packages/core/src/types.ts` funnels the package's public type exports.
- `packages/core/package.json` points both `types` and the root export's `types`
  condition at `./dist/index.d.ts`.
- `packages/core/tsconfig.json` enables declaration output under `dist`.
- `packages/core/dist/index.d.ts` re-exports the built public surface, while
  `dist/agent-os.d.ts` contains the concrete class/options declarations.
- npm-compatible package metadata includes `README.md`, while the package's
  `files` list includes `dist`, so consumers receive the landing page and the
  declarations together.

The repository does have TypeDoc, but `website/typedoc.json` targets only
`packages/agentos/src/index.ts` with `excludeExternals: true`. Pointing the Core
README at that output would falsely imply it documents the low-level Core
surface. Expanding TypeDoc to multiple packages requires a navigation and
public-surface decision and is outside this cleanup.

## Exact implementation

### 1. Add failing verifier coverage first

Edit `scripts/verify-thin-client-docs.mjs`.

Add the Core README as a fixed guidance input beside the root README:

```js
const fixedGuidancePaths = ["README.md", "packages/core/README.md"];

for (const relativePath of fixedGuidancePaths) {
	const path = join(root, relativePath);
	if (existsSync(path)) files.push({ path, relativePath });
}
```

Add path-scoped rules. Do not ban these terms across the repository because
research/tracker documents intentionally quote them as evidence.

```js
const coreReadmePaths = new Set(["packages/core/README.md"]);

// Append to forbiddenClaims.
{
	id: "core-readme-api-inventory",
	paths: coreReadmePaths,
	pattern: /^#{2,3}\s+(?:API Reference|Exported Types)\s*$/i,
},
{
	id: "core-readme-command-dirs",
	paths: coreReadmePaths,
	pattern: /\bcommandDirs\b/,
},
{
	id: "core-readme-agent-config",
	paths: coreReadmePaths,
	pattern: /`AgentConfig`/,
},
{
	id: "core-readme-agent-registry-fields",
	paths: coreReadmePaths,
	pattern: /\b(?:acpAdapter|agentPackage)\b/,
},
```

The structural heading rule is the durable guard; the named stale-claim rules
give exact before evidence and prevent the known contract copy from being
reintroduced under a different heading.

Add a Core-specific positive requirement with its own diagnostic ID (for
example `core-readme-declaration-source`) requiring these normalized fragments:

- `https://agentos-sdk.dev/docs`;
- `dist/index.d.ts`;
- `authoritative public API`; and
- `does not duplicate its method and type inventories`.

The current required-claim loop reports every missing fragment as
`required-allow-all-claim`. Generalize it into named claim groups rather than
misclassifying the Core rule:

```js
const requiredClaimGroups = [
	{ ruleId: "required-allow-all-claim", claims: requiredClaims },
	{
		ruleId: "core-readme-declaration-source",
		claims: new Map([
			[
				"packages/core/README.md",
				[
					"https://agentos-sdk.dev/docs",
					"dist/index.d.ts",
					"authoritative public API",
					"does not duplicate its method and type inventories",
				],
			],
		]),
	},
];
```

Iterate group -> file -> fragment, retaining `required-guidance-file` for a
missing file and using the group's `ruleId` for a missing fragment. Preserve all
existing permission diagnostics and counts.

Edit `scripts/verify-thin-client-docs.test.mjs` in the same step:

1. Add a valid `packages/core/README.md` fixture containing the documentation
   link and declaration-source paragraph.
2. Teach `writeValidFixture` to write `README.md` and paths beginning with
   `packages/` directly; website fixture paths keep their current mapping.
3. Add a stale Core README fixture with the two inventory headings,
   `commandDirs`, backticked `AgentConfig`, and the obsolete registry fields on
   distinct lines. Assert the four rule IDs, Core path, and representative line
   numbers.
4. Add a missing-source-statement case that asserts
   `core-readme-declaration-source`.
5. Retain `passes on the current tree`; after adding the rules but before
   changing the README, this test must fail.

Do not snapshot every current method or export in the verifier. That would
recreate the inventory in test code.

### 2. Replace the copied inventory

In `packages/core/README.md`, delete everything from `## API Reference` at line
55 through `HostDirBackendOptions` at line 204. Replace it with:

```md
## Documentation

See the [agentOS documentation](https://agentos-sdk.dev/docs) for guides.
The emitted TypeScript declarations (`dist/index.d.ts`) shipped with this package
are the authoritative public API. This README intentionally does not duplicate
its method and type inventories.
```

Preserve the current feature list and Item 39's corrected, executable Pi
quickstart. Do not alter exports or runtime behavior.

### 3. Record evidence and finish the dedicated revision

Update only Item 55's work-item and checklist rows in
`docs/thin-client-migration.md` after the gates pass. Mark completion with the
dedicated stacked JJ revision ID.

## Before and after validation

### Before behavior

- [ ] Add the path-scoped verifier rules and synthetic fixture test before
  changing the README.
- [ ] Run `node --test scripts/verify-thin-client-docs.test.mjs`. The synthetic
  stale fixture passes by finding the four expected diagnostics, while
  `passes on the current tree` fails because the live Core README still has the
  inventory.
- [ ] Run `node scripts/verify-thin-client-docs.mjs` against the unchanged
  README and record failures at current lines 55, 152, 184, and 185 for
  `core-readme-api-inventory`, `core-readme-command-dirs`,
  `core-readme-agent-config`, and `core-readme-agent-registry-fields`.
- [ ] Record declaration evidence from `agent-os.ts:95-100,454-529` and root
  exports proving the current README claims are false. No runtime execution is
  needed to prove a declaration mismatch.

### After behavior

- [ ] `node --test scripts/verify-thin-client-docs.test.mjs` passes.
- [ ] `node scripts/verify-thin-client-docs.mjs` passes and reports one more
  audited guidance file than before (Core README included).
- [ ] `pnpm --dir packages/core check-types` passes, confirming the
  authoritative source declarations remain valid.
- [ ] `pnpm --dir packages/core build` emits `dist/index.d.ts` successfully.
- [ ] A temporary `pnpm --dir packages/core pack --json --pack-destination ...`
  audit contains both `README.md` and `dist/index.d.ts`; delete the temporary
  directory afterward.
- [ ] `jj diff --check` passes.
- [ ] Item 55's tracker row is marked `done` only after the before/after
  evidence is recorded.

Focused command sequence:

```sh
node --test scripts/verify-thin-client-docs.test.mjs
node scripts/verify-thin-client-docs.mjs
pnpm --dir packages/core check-types
pnpm --dir packages/core build

tmpdir="$(mktemp -d)"
pnpm --dir packages/core pack --json --pack-destination "$tmpdir" > "$tmpdir/pack.json"
node --input-type=module - "$tmpdir/pack.json" <<'NODE'
import { readFileSync } from "node:fs";
const result = JSON.parse(readFileSync(process.argv[2], "utf8"));
const files = new Set(result.files.map(({ path }) => path));
for (const path of ["README.md", "dist/index.d.ts"]) {
	if (!files.has(path)) throw new Error(`packed package is missing ${path}`);
}
NODE
rm -rf "$tmpdir"

jj diff --check
```

No website build is required: Item 55 changes neither website source nor the
TypeDoc configuration. The existing verifier unit/live commands are already
wired into `.github/workflows/ci.yml` and `scripts/ci.sh` by Item 38.

## Tests that should not move to the sidecar

There is no client-to-sidecar test migration for Item 55. The defect is a copied
package landing-page inventory. The authoritative TypeScript declarations and
the Markdown verifier are the correct boundary; Rust or sidecar tests cannot
prevent this README from drifting.

## Dependencies and overlap

- Item 38 already supplies the verifier and CI wiring this item should extend.
- Item 39 already fixed the README quickstart; preserve it.
- Item 50 may add a compile-only public API gate. It is useful adjacent
  coverage, but Item 55 does **not** need to lock the absence of these legacy
  names in a new test: the durable requirement is that the README stops copying
  the API. If Item 50 has landed, keep its tests unchanged.
- Item 51 corrects stale package-manifest/architecture claims and may extend the
  same verifier. Rebase onto its final structure and add the Core-specific
  rules without duplicating its claim-group helper.
- Items 43, 47, and 52-54 may change public methods before Item 55 lands. Those
  changes require no README edits once the inventory is removed.

The removal can be implemented after the preceding numbered stack revisions;
it has no semantic runtime dependency on them.

## Risks and guardrails

- Do not link current website TypeDoc as a complete Core reference; it documents
  a different entrypoint.
- Do not delete the whole README. npm uses it as the package landing page, so
  keep the summary, features, executable quickstart, and guide link.
- Keep verifier rules scoped to `packages/core/README.md`; research and tracker
  files intentionally contain the stale names.
- Do not add a generated API table or a checked copy of all current exports to a
  test fixture.
- Do not make obsolete README claims true by restoring `commandDirs`,
  `AgentConfig`, `acpAdapter`, or `agentPackage`.
- Preserve unrelated changes from earlier stacked items.

## Ordered edit sequence and JJ boundary

1. In the dedicated Item 55 revision, add Core README fixture handling,
   path-scoped rules, positive requirements, and verifier tests.
2. Run the synthetic and live before checks; record the expected red live
   diagnostics.
3. Replace the README inventory with the five-line documentation section.
4. Run the focused verifier, Core type/build, package-content, and diff checks.
5. Update Item 55's tracker evidence/status and describe the revision.

Expected revision paths:

```text
packages/core/README.md
scripts/verify-thin-client-docs.mjs
scripts/verify-thin-client-docs.test.mjs
docs/thin-client-migration.md       # evidence/status only, last
docs/thin-client-research/item-55.md
```

No TypeScript production source, Rust source, sidecar/protocol file, generated
website output, package manifest, dependency, or lockfile edit is expected.
