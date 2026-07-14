# Item 49 research: remove unused Core production dependencies

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 49 tracker status.

## Recommendation

Remove six unused production dependencies from `@rivet-dev/agentos-core`:

- `@aws-sdk/client-s3`;
- `better-sqlite3`;
- `googleapis`;
- `isolated-vm`;
- `long-timeout`; and
- `minimatch`.

Delete the orphaned `long-timeout` ambient declaration and remove the one example
TypeScript configuration that explicitly includes it. Remove `better-sqlite3`
from the workspace build-script allowlist, then regenerate the root workspace
lockfile.

Delete `packages/core/pnpm-lock.yaml` rather than regenerating it. Core is a
member of the root pnpm workspace, every documented Core command resolves
through that workspace, and the package publishes only `dist`. The nested lock
is therefore neither authoritative nor shipped. It is already stale: its
importer omits current workspace dependencies while retaining links into the old
`secure-exec` tree. Keeping a second hand-maintained lock would preserve exactly
the sort of legacy package state this item is removing.

Do not move any of these packages to the sidecar. There is no client behavior to
preserve: none is imported by Core. Runtime functionality already uses sidecar
RPCs or native Node facilities. This item is deletion, not migration.

Priority: **P2**. Confidence: **high**. The source/import result is unambiguous.
The only potentially noisy part is the generated root lock diff because the
direct `better-sqlite3` dependency currently satisfies RivetKit's optional
Drizzle peer across the workspace.

## Original issue

The Item 49 tracker says Core declares unused heavy production dependencies and
an orphaned `long-timeout` declaration. This violates the thin-client boundary
in two ways:

1. the published client advertises and installs implementation stacks that it
   never calls; and
2. stale dependency metadata suggests that S3, Google APIs, SQLite, isolate
   hosting, glob policy, and long-duration scheduling are client concerns.

The actual client does none of those things through these packages. Removing
them reduces install/build surface without changing the wire protocol or moving
unneeded behavior into the sidecar.

The file/symbol/lock anchors below were re-verified in the shared working copy
at revision `930e5829fb25` (`fix(sidecar): remove implicit host path
execution`). Re-run the supplied `rg` and `pnpm why` inventory immediately
before implementation because the root lock and line numbers can move as
earlier stacked items land.

## Dependency audit and exact verdicts

`packages/core/package.json:50-64` declares fourteen production dependencies.
An exact-name scan of `packages/core/src`, `packages/core/tests`, and
`packages/core/scripts` finds no import or dynamic load of five removal
candidates. The sixth, `long-timeout`, appears only in its own ambient
declaration at `packages/core/src/cron/long-timeout.d.ts:1-12`.

| Dependency | Current declaration | Source evidence | Exact action |
| --- | --- | --- | --- |
| `@aws-sdk/client-s3` | `packages/core/package.json:53` | No Core source, test, or script import. Core's exported mock-S3 helper does not use the AWS SDK. | Remove. Do not replace. |
| `better-sqlite3` | `packages/core/package.json:58` | No Core source, test, or script import. VM persistence/runtime storage is not implemented by this client package. | Remove. Also remove the workspace build allowlist entry. |
| `googleapis` | `packages/core/package.json:59` | No Core source, test, or script import. | Remove. Do not replace. |
| `isolated-vm` | `packages/core/package.json:60` | No Core source, test, or script import. Execution is sidecar/runtime-owned. | Remove. Do not replace or move to the sidecar. |
| `long-timeout` | `packages/core/package.json:61` | No import. Its only source reference is the orphaned ambient declaration. | Remove the dependency and declaration. |
| `minimatch` | `packages/core/package.json:62` | No Core source, test, or script import. | Remove from Core. Retain the separate, legitimate `packages/posix` dependency. |

### Current dependency and lock anchors

The installed graph agrees with the literal-import audit. At the verified
revision, `pnpm -r why <name> --depth 0` reports each candidate only as a direct
Core production dependency, except that `minimatch@10.2.5` also appears as a
legitimate `@rivet-dev/agentos-posix` dev dependency.

| Dependency | Core manifest | Root Core importer | Root package/snapshot anchors | Nested Core lock importer |
| --- | --- | --- | --- | --- |
| `@aws-sdk/client-s3` | `package.json:53`, `^3.1019.0` | `pnpm-lock.yaml:2426-2428`, resolved `3.1020.0` | package at 3786; snapshot at 9175 | lines 11-13, resolved `3.1024.0` |
| `better-sqlite3` | line 58, `^12.8.0` | lines 2441-2443, resolved `12.8.0` | package at 6035; snapshot at 11873; Drizzle/RivetKit peer context at 12453 and nearby peer-suffixed snapshots | lines 17-19 |
| `googleapis` | line 59, `^144.0.0` | lines 2444-2446, resolved `144.0.0` | package at 7001; snapshot at 13000 | lines 23-25 |
| `isolated-vm` | line 60, `^6.0.0` | lines 2447-2449, resolved `6.1.2` | package at 7208; snapshot at 13196 | lines 26-28 |
| `long-timeout` | line 61, `^0.1.1` | lines 2450-2452, resolved `0.1.1` | package at 7327; snapshot at 13308 | lines 29-31 |
| `minimatch` | line 62, `^10.2.4` | lines 2453-2455, resolved `10.2.5` | shared package at 7425; snapshot at 13383 | lines 32-34 |

The nested lock resolving a newer AWS SDK than the authoritative root lock is
additional proof that it is an ignored parallel lock, not a release artifact to
preserve.

The retained production dependencies all have a concrete shipped use:

- `@agentos-software/common` and `@agentos-software/manifest` are used by
  `src/default-software.ts` and package input handling. The default-package
  exception explicitly allows this TypeScript package-manager behavior.
- `@rivet-dev/agentos-sidecar` resolves the published native sidecar binary in
  `src/sidecar/binary.ts`.
- `@rivetkit/bare-ts` is used by the generated BARE protocol module.
- `@rivet-dev/agentos-runtime-core` supplies the framed sidecar client and
  shared descriptor/config/error types.
- `@xterm/headless` is imported by `src/test/terminal-harness.ts:6`. That helper
  is shipped through the public `./test/runtime` subpath declared at
  `package.json:37-41`, so it must remain a production dependency.
- `zod` and `zod-to-json-schema` implement the intentionally client-owned host
  tool construction/validation boundary.

Do not broaden Item 49 into reclassifying `sandbox-agent` or `vitest`. They are
separate package-surface questions because Core currently ships repo test
subpaths that import them. This item has high confidence only for the six exact
dependencies above.

## The `long-timeout` declaration is dead

`packages/core/src/cron/long-timeout.d.ts` declares a module-shaped API but no
production file imports that module. Current cron host wakeup uses the normal
Node timer in `packages/core/src/cron/cron-manager.ts:21-78`:

```ts
const MAX_TIMER_DELAY_MS = 2_147_483_647;

class TimerCronAlarmDriver implements CronAlarmDriver {
  private timer: ReturnType<typeof setTimeout> | null = null;

  set(alarm: SidecarCronAlarm, wake: (generation: number) => Promise<void>): void {
    const delay = Math.min(
      MAX_TIMER_DELAY_MS,
      Math.max(0, alarm.nextAlarmMs - Date.now()),
    );
    this.timer = setTimeout(() => {
      if (delay === MAX_TIMER_DELAY_MS) {
        this.set(alarm, wake);
        return;
      }
      // Wake the sidecar-owned cron state.
    }, delay);
  }
}
```

This chains the host alarm at Node's maximum timer delay and retains the
legitimate actor/host wake hook documented by item 11. It does not depend on the
`long-timeout` package. Do not remove or sidecar-ize this host alarm as part of
Item 49; only delete the unused declaration and package.

Core's `tsconfig.json:11` includes all of `src/**/*`, so the ambient declaration
is currently compiled even though it is unused. In addition,
`packages/secure-exec-example-ai-agent-type-check/tsconfig.json:16` explicitly
reaches into Core to include the declaration. That cross-package include is the
only external source reference and must be removed with the file.

## Exact edits

### `packages/core/package.json`

Delete only the six dependency entries listed above. Preserve the retained
dependencies and the committed package version `0.0.1`. The resulting block is:

```json
"dependencies": {
  "@agentos-software/common": "workspace:*",
  "@agentos-software/manifest": "workspace:*",
  "@rivet-dev/agentos-sidecar": "workspace:*",
  "@rivetkit/bare-ts": "^0.6.2",
  "@rivet-dev/agentos-runtime-core": "workspace:*",
  "@xterm/headless": "^6.0.0",
  "zod": "^4.1.11",
  "zod-to-json-schema": "^3.25.2"
}
```

Do not reorder or otherwise rewrite the retained block; a deletion-only manifest
diff makes the dependency audit reviewable.

### `packages/core/src/cron/long-timeout.d.ts`

Delete the file in full. There is no replacement import or local type. Node's
normal `setTimeout` type already describes `cron-manager.ts`.

### `packages/secure-exec-example-ai-agent-type-check/tsconfig.json`

Change:

```json
"include": ["src/**/*.ts", "../core/src/cron/long-timeout.d.ts"]
```

to:

```json
"include": ["src/**/*.ts"]
```

No other compiler option needs to change.

### `pnpm-workspace.yaml`

Delete `better-sqlite3` from `onlyBuiltDependencies` at the current line 46.
After the Core direct dependency disappears, no workspace manifest deliberately
installs it. Leaving the entry would preserve a misleading native-build policy
for a package the workspace no longer selects.

### `packages/core/pnpm-lock.yaml`

Delete this file in full. Evidence that it is a stale parallel source of truth:

- `packages/core` is explicitly listed in the root `pnpm-workspace.yaml`;
- the root pins `pnpm@10.13.1` and owns `pnpm-lock.yaml`;
- every repository command found for Core uses `pnpm --dir packages/core ...`
  from within that workspace, with no `--ignore-workspace` path;
- the package publishes only `dist`, so consumers never receive the lock;
- its importer omits current production workspace dependencies; and
- it still contains `link:../../../secure-exec/...` entries.

Do not spend review effort mechanically pruning a 200 KB lockfile that pnpm does
not use in this repository.

### Root `pnpm-lock.yaml`

After the manifest and workspace-policy edits, run the pinned workspace pnpm to
regenerate the root lock:

```sh
pnpm install --lockfile-only
```

Review the generated diff rather than hand-editing it. It must remove the six
entries from the `packages/core` importer at current lines 2426-2455 and prune
dependency subtrees that no other importer reaches. The following direct
changes are guaranteed by the manifest edit:

- delete Core's importer entries for all six packages;
- delete the root package and snapshot entries for `@aws-sdk/client-s3`,
  `googleapis`, `isolated-vm`, and `long-timeout` when the regenerated graph
  confirms no other importer/edge;
- delete `better-sqlite3@12.8.0`, prune any of its native dependency subtree
  that is now unreachable, and rewrite the Drizzle/RivetKit snapshots so they
  no longer resolve that optional peer;
- retain `minimatch@10.2.5`, because the `packages/posix` importer at current
  lines 2651-2665 still selects it, while removing only Core's edge;
- prune shared transitive packages only when no remaining importer or snapshot
  reaches them. Do not remove AWS/Google/native-build utilities by name without
  letting pnpm prove they are unreachable.

A large peer-suffix diff is expected. The current direct `better-sqlite3`
dependency causes workspace RivetKit snapshots to resolve as
`rivetkit@...(...)(better-sqlite3@12.8.0)(...)`, and Drizzle's snapshot currently
has `better-sqlite3` as an optional dependency. Once no workspace manifest
provides that optional peer, pnpm will rewrite those contexts without the
SQLite suffix. That churn is caused by Item 49 and should not be reverted merely
to keep the lock diff small.

Conversely, do not assert that every `minimatch` string vanishes from the root
lock. `packages/posix/package.json:31` legitimately declares its own
`minimatch` dev dependency. Validate Core's importer and manifest, not global
string absence.

## Before and after validation

This item has no runtime behavior to preserve, but the thin-client package
boundary is testable. Add a focused manifest/source regression test before the
deletion so there is executable evidence that fails on the Item 49 parent and
passes after it. Compilation, consumer typechecking, a packed-package smoke
test, and frozen-lock verification then prove that deleting the declarations
does not remove real behavior.

### Before test that fails on the current parent

Add `packages/core/tests/thin-client-dependencies.test.ts` before changing the
manifest. It should use paths derived from `import.meta.dirname` so it works
both through `pnpm --dir packages/core` and Turbo. Keep the test declarative and
bounded to Item 49:

```ts
import { access, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { describe, expect, test } from "vitest";

const coreRoot = resolve(import.meta.dirname, "..");
const repoRoot = resolve(coreRoot, "../..");
const removedDependencies = [
  "@aws-sdk/client-s3",
  "better-sqlite3",
  "googleapis",
  "isolated-vm",
  "long-timeout",
  "minimatch",
] as const;

describe("thin-client production dependencies", () => {
  test("does not install unused runtime implementation stacks", async () => {
    const manifest = JSON.parse(
      await readFile(resolve(coreRoot, "package.json"), "utf8"),
    ) as { dependencies?: Record<string, string> };

    for (const name of removedDependencies) {
      expect(manifest.dependencies).not.toHaveProperty(name);
    }
  });

  test("does not compile the orphaned long-timeout declaration", async () => {
    await expect(
      access(resolve(coreRoot, "src/cron/long-timeout.d.ts")),
    ).rejects.toMatchObject({ code: "ENOENT" });
  });

  test("does not retain obsolete dependency-install metadata", async () => {
    const workspace = await readFile(
      resolve(repoRoot, "pnpm-workspace.yaml"),
      "utf8",
    );
    expect(workspace).not.toMatch(/^\s*- better-sqlite3\s*$/m);
    await expect(access(resolve(coreRoot, "pnpm-lock.yaml"))).rejects.toMatchObject(
      { code: "ENOENT" },
    );
  });
});
```

Run it alone against the parent:

```sh
pnpm --dir packages/core exec vitest run \
  tests/thin-client-dependencies.test.ts --reporter=verbose
```

It must fail before the implementation because all six manifest properties,
the ambient declaration, the `better-sqlite3` build allowlist entry, and the
nested Core lock are present. Record that failure in the tracker before
changing production metadata. This test intentionally checks the declared and
installed client surface, not whether a package happened to be loaded during a
particular runtime test.

### Before checklist

- [ ] `tests/thin-client-dependencies.test.ts` is added first and fails against
  the Item 49 parent for the present dependency/declaration/install metadata.
- [ ] Record the exact-name audit over `packages/core/src`,
  `packages/core/tests`, and `packages/core/scripts`, excluding the new
  boundary test that necessarily names the denied dependencies. It must find
  none of the six packages except `long-timeout` in its own declaration before
  deletion.
- [ ] Record `pnpm -r why <dependency> --depth 0` for all six. It must identify
  Core as the only direct selector for five packages and Core plus POSIX for
  `minimatch` (production in Core, dev-only in POSIX).
- [ ] Record that `cron-manager.ts` compiles against native `setTimeout` and
  contains no `long-timeout` import.
- [ ] Record that `packages/secure-exec-example-ai-agent-type-check/tsconfig.json`
  is the only source outside Core that references the declaration.
- [ ] Record the retained-dependency audit, especially the shipped
  `@xterm/headless` `./test/runtime` path, so the cleanup does not overreach.

One reproducible audit command is:

```sh
rg -n -F \
  -e '@aws-sdk/client-s3' \
  -e 'better-sqlite3' \
  -e 'googleapis' \
  -e 'isolated-vm' \
  -e 'long-timeout' \
  -e 'minimatch' \
  --glob '!**/thin-client-dependencies.test.ts' \
  packages/core/src packages/core/tests packages/core/scripts
```

Before the edit, its sole result should be the declaration file. After the edit,
it should return no matches (exit status 1). The focused boundary test is
excluded only from this import/use inventory; it is run separately and must
continue naming all six forbidden dependencies.

Use this separate manifest/installed-graph inventory so the literal source
search is not mistaken for evidence that the packages are absent:

```sh
rg -n -F \
  -e '"@aws-sdk/client-s3"' \
  -e '"better-sqlite3"' \
  -e '"googleapis"' \
  -e '"isolated-vm"' \
  -e '"long-timeout"' \
  -e '"minimatch"' \
  --glob package.json .

for name in @aws-sdk/client-s3 better-sqlite3 googleapis isolated-vm long-timeout minimatch; do
  pnpm -r why "$name" --depth 0
done
```

### After checklist

- [ ] `pnpm --dir packages/core exec vitest run
  tests/thin-client-dependencies.test.ts --reporter=verbose` passes.
- [ ] `pnpm install --lockfile-only` completes and changes only dependency-driven
  root lock content in addition to already-stacked parent changes.
- [ ] `pnpm install --frozen-lockfile --ignore-scripts` accepts the regenerated
  root lock.
- [ ] `pnpm --dir packages/core build` passes.
- [ ] `pnpm --dir packages/core check-types` passes without the ambient module.
- [ ] `pnpm --filter @rivet-dev/agentos-example-ai-agent-type-check check-types`
  passes without its cross-package include.
- [ ] The built root entry and shipped test runtime both import:

  ```sh
  node --input-type=module -e \
    'await import("./packages/core/dist/index.js"); await import("./packages/core/dist/test/runtime.js")'
  ```

- [ ] A packed tarball contains none of the six dependency declarations. For
  example:

  ```sh
  tmpdir="$(mktemp -d)"
  pnpm --dir packages/core pack --pack-destination "$tmpdir"
  archive="$(find "$tmpdir" -name '*.tgz' -print -quit)"
  tar -xOf "$archive" package/package.json | node --input-type=module -e '
    let input = "";
    for await (const chunk of process.stdin) input += chunk;
    const pkg = JSON.parse(input);
    const removed = [
      "@aws-sdk/client-s3", "better-sqlite3", "googleapis",
      "isolated-vm", "long-timeout", "minimatch",
    ];
    for (const name of removed) {
      if (pkg.dependencies?.[name]) throw new Error(`packed dependency remains: ${name}`);
    }
  '
  rm -rf "$tmpdir"
  ```

- [ ] `node scripts/verify-fixed-versions.mjs` passes.
- [ ] `packages/core/pnpm-lock.yaml` is absent and
  `rg --files -g pnpm-lock.yaml` reports only the root lock.
- [ ] The `packages/core` root-lock importer contains none of the six removed
  edges; `minimatch@10.2.5` remains reachable through `packages/posix`, while
  `better-sqlite3` and its RivetKit/Drizzle peer suffix disappear.
- [ ] Item 49 is marked complete only after all preceding checks have passed.

## Validation commands

Run these from the repository root after the deletion and lock regeneration:

```sh
pnpm install --lockfile-only
pnpm install --frozen-lockfile --ignore-scripts

pnpm --dir packages/core exec vitest run \
  tests/thin-client-dependencies.test.ts --reporter=verbose
pnpm --dir packages/core build
pnpm --dir packages/core check-types
pnpm --filter @rivet-dev/agentos-example-ai-agent-type-check check-types

node --input-type=module -e \
  'await import("./packages/core/dist/index.js"); await import("./packages/core/dist/test/runtime.js")'

pnpm check-types
pnpm build
node scripts/verify-fixed-versions.mjs

test ! -e packages/core/pnpm-lock.yaml
test "$(rg --files -g pnpm-lock.yaml | sort)" = "pnpm-lock.yaml"
```

Also run the packed-manifest assertion in the after checklist. It is the
consumer-facing proof that the six packages no longer install with Core; build
and typecheck alone cannot prove package metadata.

## Dependencies and risks

Item 49 has no protocol or runtime implementation dependency. It should still
land in tracker order on the completed preceding-item revision so its generated
root-lock diff can be reviewed relative to one known parent. The current cron
alarm implementation may include earlier Item 37 changes, but this item depends
only on the stable fact that it uses Node's built-in timer and must not absorb
cron behavior.

1. **Expected root-lock churn.** Removing `better-sqlite3` changes RivetKit and
   Drizzle peer resolution suffixes across many importers. Review whether every
   change is a consequence of removing that optional peer; do not demand a tiny
   lock diff.
2. **Unrelated shared-working-copy lock edits.** The workspace can already
   contain lock changes from earlier stacked items. Regenerate on the Item 49
   parent and inspect the revision-relative diff so those changes are preserved,
   not overwritten or claimed by this revision.
3. **Do not delete all minimatch resolutions.** POSIX still uses it.
4. **Do not remove `@xterm/headless`.** It is reachable through a shipped Core
   export even though the source lives under `src/test`.
5. **Do not move dead packages into the sidecar.** Absence of an import means
   there is no behavior to migrate.
6. **Keep the cron wake hook.** The ordinary timer is host scheduling state that
   wakes sidecar-owned durable cron state; Item 49 removes only its obsolete
   declaration package.
7. **No public API or protocol change is intended.** A generated BARE fixture or
   Rust edit indicates accidental scope expansion.
8. **Do not combine Item 50 or 51 cleanup.** The deprecated software descriptor
   and stale guidance are separately tracked even if manifest/doc review makes
   them visible while implementing this deletion.
9. **No secure-exec mirror hand edit.** This does not change a shimmed public
   API, and the generated compatibility shim depends on AgentOS Core as a whole,
   not on these transitive declarations. If a standard mirror verification is
   run, regenerate with `node scripts/generate-secure-exec-mirror.mjs`; never
   patch the mirror directly.

## Bounded JJ revision

Create one dedicated stacked revision for Item 49, based on the completed prior
item. Do not switch the shared working copy with `jj edit`. The implementation
revision should be bounded to:

- `packages/core/package.json`;
- new `packages/core/tests/thin-client-dependencies.test.ts`;
- deletion of `packages/core/src/cron/long-timeout.d.ts`;
- `packages/secure-exec-example-ai-agent-type-check/tsconfig.json`;
- `pnpm-workspace.yaml`;
- deletion of `packages/core/pnpm-lock.yaml`;
- generated `pnpm-lock.yaml` changes caused by this manifest/policy edit;
- `docs/thin-client-migration.md`, after validation, to check the Item 49 before,
  after, and completion boxes and mark its work-item row `done`; and
- this research note if it has not already landed in the research stack.

Suggested description:

```text
chore(core): remove unused production dependencies
```

Before moving the bookmark, inspect the revision-relative diff and confirm there
are no Rust, sidecar, protocol, generated-protocol, runtime behavior, or unrelated
package-manifest changes.
