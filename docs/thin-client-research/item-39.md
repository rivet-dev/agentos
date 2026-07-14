# Item 39 research — executable Pi package quickstart

Status: implementation-ready research only. **Priority: P1. Fix confidence:
high.** This note does not modify runtime code, tests, or the Item 39 tracker
status.

## Finding

Item 39 is a TypeScript package-documentation defect, not a sidecar or Rust
client defect. The broken snippet is specifically the `## Quick Start` block in
`packages/core/README.md:19-42`:

- it installs `@agentos-software/pi`, but never imports its default export;
- it calls `AgentOs.create()` with no `software` override;
- it then calls `createSession("pi")`, although no Pi package was projected.

The root `README.md:30-57` is already explicit: it imports Pi and passes it in
`software`. Do not change that separate block as part of Item 39.

The production behavior is correct and should not change:

| Stage | Current symbol/anchor | Current behavior |
|---|---|---|
| Pi descriptor | `registry/agent/pi/src/index.ts:1-5` | The package default-exports `{ packagePath }`, pointing at its built `.aospkg`. |
| Allowed TypeScript defaults | `packages/core/src/default-software.ts:5-12` (`resolveDefaultSoftware`) | A bare `AgentOs.create()` adds only the non-agent `common` bundle. It does not choose Pi. |
| Client validation | `packages/core/src/agent-os.ts:672-690` (`normalizePackageRef`) | An explicit package descriptor is reduced to its path; the client does not read its manifest. |
| Client forwarding | `packages/core/src/agent-os.ts:1368-1404` (`AgentOs.create`) and `1482-1490` (`client.initializeVm`) | Explicit Pi is merged with the allowed package-manager defaults and forwarded as an ordinary `packages` input. |
| ACP request | `packages/core/src/agent-os.ts:2702-2749` (`AgentOs.createSession`) | The client forwards only the caller's agent name and explicit session fields. |
| Sidecar resolution | `crates/agentos-sidecar-core/src/engine.rs:72-91` (`resolve_agent`) | The shared sidecar core resolves the name from projected state and returns the stable unknown-agent error when no projected package supplies `agent.acpEntrypoint`. |

This is therefore a documentation/example wiring defect, not missing sidecar
functionality and not a Rust parity defect.

There is no useful default to move to the sidecar here. Pi is an explicit caller
package choice. Making every VM contain Pi would violate the thin-client rule in
the other direction by turning an application package choice into a runtime
default. The fix is to make the example forward its explicit package input.

`@mariozechner/pi-coding-agent` should also disappear from the install command.
It is already a direct dependency of `@agentos-software/pi` at
`registry/agent/pi/package.json:31-34`; users should not have to install the
adapter implementation twice or coordinate its version manually.

## Recommended source of truth

Reuse `examples/quickstart/agent-session/index.ts`; do not introduce another
near-identical Pi example. It is already a workspace package, has a real
`check-types` command, and currently projects Pi explicitly. Narrow it from the
current three-agent selector to the Pi-only flow advertised by the core README.

Wrap the runnable portion in the repository's existing snippet markers:

```ts
// docs:start core-readme-quickstart
import pi from "@agentos-software/pi";
import { AgentOs } from "@rivet-dev/agentos-core";

const apiKey = process.env.ANTHROPIC_API_KEY;
if (!apiKey) {
	throw new Error("ANTHROPIC_API_KEY is required");
}

const vm = await AgentOs.create({ software: [pi] });

try {
	const { sessionId } = await vm.createSession("pi", {
		env: { ANTHROPIC_API_KEY: apiKey },
	});

	try {
		const { text } = await vm.prompt(
			sessionId,
			"Write a hello world in TypeScript",
		);
		console.log(text);
	} finally {
		await vm.closeSession(sessionId);
	}
} finally {
	await vm.dispose();
}
// docs:end core-readme-quickstart
```

The explicit API-key check avoids forwarding an `undefined` value disguised by
a TypeScript non-null assertion. The nested cleanup is deliberate: a failed
prompt still closes its live ACP session, and a failed
create-session/prompt/close still releases the VM. The README code fence should
contain the exact region content without the two marker lines. This keeps the
copy-paste path simple while allowing an automated equality check.

Do not add `common` to this snippet. `AgentOs.create()` currently supplies the
TypeScript package manager's allowed default package bundle, while Pi is the
explicit non-default package that this regression is about. Passing only
`software: [pi]` demonstrates the required contract without duplicating the
package manager's default list at the call site.

## Exact edits

### `packages/core/README.md`

Replace lines 21-42 as one unit:

1. Install `@rivet-dev/agentos-core` and `@agentos-software/pi`; remove the
   separate `@mariozechner/pi-coding-agent` install and its misleading comment.
2. Import the default `pi` package reference.
3. call `AgentOs.create({ software: [pi] })`.
4. Fail clearly when `ANTHROPIC_API_KEY` is absent; otherwise pass the concrete
   string in session `env`, destructure the documented `{ text }` prompt result,
   print it, and use awaited `try/finally` cleanup.
5. Keep the TypeScript fence byte-for-byte equal to the marked checked-example
   region after normalizing final newlines.

Do not edit the hand-maintained API inventory below the quickstart; Item 55 owns
that separate defect.

### `examples/quickstart/agent-session/index.ts`

Replace the current multi-agent setup at lines 1-43 with the marked Pi-only code
above. Delete the `SoftwareInput`, Claude, and OpenCode imports, the three-package
`software` array, the agent selector, event-log boilerplate, and the conditional
empty env assembly. Those abstractions make the example broader without helping
the one package it actually selects.

### `examples/quickstart/agent-session/README.md`

Update lines 2-21 to say the example runs Pi, projects the Pi package explicitly,
requires `ANTHROPIC_API_KEY`, and prints the prompt result. Remove the claims that
the same checked file selects Claude or OpenCode.

### `examples/quickstart/agent-session/package.json` and `pnpm-lock.yaml`

Reduce this example's production dependencies to the only packages the checked
file imports. Its current lines 10-22 are copied generic-quickstart inventory;
after narrowing `index.ts`, every entry except Core and Pi is unused:

```json
"dependencies": {
  "@agentos-software/pi": "workspace:*",
  "@rivet-dev/agentos-core": "workspace:*"
}
```

Retain `@types/node`, `tsx`, and `typescript` as development dependencies. Run a
lockfile-only install/update so the workspace importer in `pnpm-lock.yaml`
matches. Do not remove shared packages globally merely because this one example
no longer imports them.

### `packages/core/tests/readme-quickstart.test.ts`

Add the acceptance file named by the tracker. It should have three fast tests
(or two tests with the two execution cases parameterized):

1. Extract the first `typescript` fence under `## Quick Start` in
   `packages/core/README.md`, extract `docs:start/end core-readme-quickstart`
   from `examples/quickstart/agent-session/index.ts`, and require equality after
   trimming only the final newline. Missing/duplicate fences or markers must be
   explicit failures, not an empty match.
2. Execute the extracted README program on its success path against deterministic
   injected test doubles for `AgentOs`, `pi`, `process`, and `console`. Remove
   the two known one-line imports, transpile the remaining TypeScript with the
   already-present `typescript` development dependency
   (`packages/core/package.json:103`), and run it in an `AsyncFunction`. Reject
   any other import instead of silently stripping arbitrary code. The
   fake VM must model the relevant runtime invariant: `createSession("pi")`
   throws `unknown agent type: pi` unless the exact injected Pi descriptor was
   present in `AgentOs.create({ software })`. After execution, assert this order
   and data:

   - `create` received `software: [pi]`;
   - `createSession` received `"pi"` and the injected API key;
   - `prompt` received the returned session ID and expected prompt;
   - `closeSession` was awaited before `dispose`;
   - the returned `text` was logged.
3. Execute the same extracted program with `prompt` rejecting. Assert that the
   original prompt error propagates, `closeSession` completes, and only then
   `dispose` completes. This validates the cleanup behavior shown to users rather
   than merely asserting that the method names occur in the snippet.

Keep this default test transport-free. It executes the actual published snippet
and proves the regression's data flow without booting the built Pi package or
calling an external model API on every PR. Do not mock or change production
`AgentOs` modules globally; inject the two imports only into the extracted
snippet's evaluation scope.

The real integration proof already exists in
`packages/core/tests/pi-headless.test.ts`: `createPiVm` at lines 49-55 passes
`[common, pi]`, the first case at lines 85-119 initializes the real Pi SDK ACP
adapter, and the following cases exercise prompts through LLMock and clean up.
Run that focused file in the expensive validation phase instead of copying its
model/bootstrap scaffolding into the documentation test.

## Before and after validation

### Before evidence

Add the final extraction/execution harness first on the Item 38 parent, before
editing either snippet. Its success-path test must fail because the current
README calls `AgentOs.create()` without Pi; the fake reaches
`createSession("pi")` and throws `unknown agent type: pi`. Record this exact test
name, command, and observed failure in the Item 39 tracker checklist. Do not
weaken or invert the final assertion just to capture the before evidence.

```sh
pnpm --dir packages/core exec vitest run tests/readme-quickstart.test.ts
```

Also record the existing multi-agent example's typecheck before replacement;
that is a compatibility baseline, not proof that the README works:

```sh
pnpm --dir examples/quickstart/agent-session check-types
```

Current-checkout audit on 2026-07-14: this command exits 2 before checking the
example because `examples/quickstart/agent-session/node_modules` is absent and
TypeScript cannot resolve `@agentos-software/opencode`. That is an installation
precondition, not Item 39's before failure. Run the baseline after the normal
workspace dependency install has created this nested workspace package's links;
do not record the missing-link error as behavioral evidence.

### After evidence

Run the same fast test after synchronizing the README and example. It must execute
through prompt and both cleanup calls, its failure case must still clean up, and
its source-equality assertion prevents the README from drifting back to an
unprojected package.

```sh
pnpm --dir packages/core exec vitest run tests/readme-quickstart.test.ts
pnpm --dir examples/quickstart/agent-session check-types
pnpm --dir packages/core check-types
```

Then run the existing real adapter proof as expensive validation. It requires
built registry artifacts and the native sidecar, but no real API key because it
uses LLMock:

```sh
AGENTOS_E2E_FULL=1 pnpm --dir packages/core exec vitest run tests/pi-headless.test.ts
```

Finally run `git diff --check` and update all three Item 39 checkboxes plus its
status row in `docs/thin-client-migration.md` only after the fast and real tests
pass.

## Dependencies and risks

- **Item 55 path overlap:** it also owns `packages/core/README.md`. Land Item 39
  first or rebase Item 55 and preserve the checked quickstart block.
- **Item 51 documentation verifier:** if its scan later includes package READMEs,
  retain the explicit-package positive claim and point it at this source-equality
  test rather than creating a second snippet verifier.
- **Built artifacts:** the real Pi test imports
  `registry/agent/pi/dist/package.aospkg`; it must be built before the expensive
  run. The fast documentation test must not depend on that artifact.
- **Workspace install:** the nested example currently has no local
  `node_modules`, so its standalone `check-types` command cannot resolve its
  workspace dependencies. Run the normal workspace install before claiming the
  before/after typecheck result; this is independent of the quickstart defect.
- **No network in default CI:** executing the checked file directly would call a
  model and require loopback exception/model setup for LLMock. The injected
  execution harness is intentional; actual sidecar behavior stays covered by
  `pi-headless.test.ts`.
- **Cleanup assertions:** recording only invocation order is not enough if
  promises are not awaited. Make fake close/dispose promises complete on
  separate microtasks and assert their completion flags, so removing `await`
  fails.
- **Manifest cleanup scope:** the same copied dependency inventory appears in
  sibling `examples/quickstart/*/package.json` files. Item 39 should clean only
  the checked agent-session package. A repository-wide example-manifest cleanup
  is a separate item and must not expand this revision.
- **Scope:** no Rust client, protocol, sidecar, package projection, or default
  software production code should change for Item 39. Confidence is high.

## Dedicated stacked `jj` revision

Create one new revision on top of the completed Item 38 revision, without moving
the shared workspace to another bookmark. Suggested description:

```text
docs(core): execute the explicit Pi quickstart
```

The revision's intended path scope is exactly:

- `packages/core/README.md`
- `packages/core/tests/readme-quickstart.test.ts`
- `examples/quickstart/agent-session/index.ts`
- `examples/quickstart/agent-session/README.md`
- `examples/quickstart/agent-session/package.json`
- `pnpm-lock.yaml`
- `docs/thin-client-migration.md`

Before creating/editing it, verify `pwd` and `jj log -r @`; after validation,
describe the revision and advance the existing stack bookmark to its tip. Do not
create a per-item bookmark or PR.
