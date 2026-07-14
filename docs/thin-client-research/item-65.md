# Item 65 research — preserve structured TypeScript cleanup errors

Status: implementation-ready research only. This note does not modify
production code, tests, or the Item 65 tracker status.

## Recommendation

Replace the three remaining TypeScript cleanup errors that join child messages
into a new plain `Error` with contextual `AggregateError` objects:

- `AgentOsSidecarClient.disposeSession`;
- `AgentOsSidecarClient.dispose`; and
- `__disposeAllSharedSidecarsForTesting`.

Keep the original child `Error` objects in deterministic cleanup order. A
human-readable joined string may remain in `AggregateError.message` and in the
session lifecycle's string-only `lastError` field, but it must no longer be the
only representation of the failures.

Do not move this aggregation into the sidecar. The sidecar remains authoritative
for each operation's stable rejection code and protocol context. These three
functions coordinate multiple host transports, sessions, or sidecar handles,
so only the TypeScript host has the complete set of failures to aggregate. This
is thin transport/lifecycle bookkeeping, not duplicated runtime policy.

Priority: **P2**. Confidence: **high**. A repository-wide search finds exactly
three remaining TypeScript cleanup throw sites where a collected error array is
replaced by joined message text. The
lease-owner and injected in-process transport sites named by the tracker were
already converted to `AggregateError` by revision `d6fd5890b296` (`fix(client):
close sidecar sessions reliably`), so they need regression coverage rather than
another production rewrite.

## Current inventory

| Cleanup owner | Current behavior | Item 65 action |
|---|---|---|
| `AgentOsSidecarClient.disposeSession`, `packages/core/src/sidecar/rpc-client.ts:1338-1374` | Tries every VM and the session transport, then sets `lastError` to joined messages and throws a new plain `Error`. VM and transport error identities, codes, and response frames are lost. | Replace the thrown value with `AggregateError(errors, contextualMessage)`; retain the display string in `lastError`. |
| `AgentOsSidecarClient.dispose`, `packages/core/src/sidecar/rpc-client.ts:1376-1395` | Tries every session, then joins each session error message into a new plain `Error`. | Throw one client-level `AggregateError`. Preserve the per-session aggregates as its children instead of recursively flattening them. |
| `__disposeAllSharedSidecarsForTesting`, `packages/core/src/agent-os.ts:3537-3553` | Tries every cached shared sidecar, clears the cache, then replaces all disposal errors with one joined-message `Error`. | Throw `AggregateError` containing the exact sidecar errors. |
| `AgentOsSidecar.disposeOnce`, `packages/core/src/agent-os.ts:3495-3517` | Tries every active lease and already throws `AggregateError(errors, "failed to dispose sidecar …")`. Failed leases remain active for retry. | Already compliant; no production edit. Cover it through the public lease/injected-transport regression described below. |
| `createInProcessSidecarTransport.disposeTransport`, `packages/core/src/agent-os.ts:3691-3706` | Tries every registered VM admin, retains failed admins for retry, and already throws `AggregateError(errors, "failed to dispose sidecar session")`. | Already compliant; no production edit. Cover it through the same regression. |
| `NativeSidecarKernelProxy.disposeOnce`, `packages/core/src/sidecar/rpc-client.ts:296-368` | Already retains original teardown failures in `AggregateError`; remote disposal remains retryable. | Adjacent and compliant. Strengthen the existing retry test to assert child identity. |
| Rust `AgentOs::shutdown`, `crates/client/src/agent_os.rs:457-520` | Sends one authoritative `CloseSession` request and only then releases local routes and the VM lease. It does not aggregate independent sidecar requests, so there is no Rust analogue to the TypeScript loss. | No behavioral change. Keep the sidecar rejection as the typed `ClientError` source. |
| Rust `AgentOsSidecar::dispose`, `crates/client/src/sidecar.rs:230-274` | Contains an always-empty `Vec<String>` plus an unreachable branch and stale comment documenting TypeScript's plain joined `Error`. Actual lease cleanup happens through `AgentOs::shutdown`. | Delete the dead vector/branch and return `Ok(())` after pool removal. This is a no-behavior cleanup that prevents the fixed TypeScript bug from remaining the documented parity target. |
| Wire/runtime rejection mapping, `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:809-812`, `packages/runtime-core/src/protocol-client.ts:234-241`, and `packages/runtime-core/src/sidecar-errors.ts:8-26` | The wire supplies stable `code` plus `message`; the runtime wraps it in `SidecarRequestRejected` with `requestId`, `ownership`, and the complete response frame. | No protocol/runtime change. Preserve this object by identity when host cleanup aggregates it. |
| Native/browser/shared sidecar cleanup enums | `SidecarError::Cleanup`, `BrowserSidecarError::Cleanup`, and `AcpCoreError::Cleanup` already preserve ordered internal causes and emit `cleanup_failed`. | No sidecar change. These aggregate failures inside one authoritative sidecar operation; they cannot see multiple host sessions/transports. |
| Public core docs, `website/src/content/docs/docs/core.mdx:38-46` | Explain shared and explicit sidecars but do not state disposal's try-all/structured failure contract. | Add one short sentence after the explicit-sidecar example stating that teardown attempts every owned resource and rejects with `AggregateError` containing the original failures. |

The tracker wording is therefore partly stale. “Lease” means the
`AgentOsSidecar` active-lease cleanup loop, not the warning-only catch after a
failed lease creation. The latter, plus the warning-only startup cleanup catches
in `AgentOs.create`, do not join an error collection into a string. Changing
which primary error those paths return is a separate cleanup-transaction audit
and should not be silently folded into this bounded revision.

## Cross-layer ownership audit

The protocol's `RejectedResponse` carries one sidecar operation's stable `code`
and `message`. `ProtocolClient.validateResponse` converts that frame to
`SidecarRequestRejected`; the object retains `code`, `requestId`, `ownership`,
and the complete response. `toError` in the core RPC client returns an existing
`Error` unchanged. Therefore no information is lost on the wire or during the
first TypeScript mapping: only the three final plain-`Error` constructions lose
the structured object.

Native, browser, and shared ACP sidecar code already use ordered cleanup error
variants (`SidecarError::Cleanup`, `BrowserSidecarError::Cleanup`, and
`AcpCoreError::Cleanup`) and map them to `cleanup_failed`. Those variants cover
multiple failures that occur *inside one sidecar operation*. Adding a wire list
of causes would not solve this item: the TypeScript host is combining multiple
already-completed requests/transports and is the only layer that knows all of
those children. Host aggregation is consequently required lifecycle routing,
not client-owned runtime policy.

Rust deliberately uses one wire session per VM and `AgentOs::shutdown` sends a
single `CloseSession` transaction. It has no corresponding multi-session host
aggregate to implement. `crates/client/src/sidecar.rs:248-273` nevertheless has
an always-empty `errors: Vec<String>` and an unreachable fallback whose comment
describes the TypeScript bug. Delete that dead scaffold in this revision so the
Rust client remains simple and does not preserve obsolete parity guidance. Do
not add a Rust aggregate error variant merely to mirror JavaScript's standard
`AggregateError`.

## Exact remaining failures

### Session cleanup discards VM and transport failures

`AgentOsSidecarClient.disposeSession` currently collects the right objects in
the right order:

1. one error for each VM entry, in `Map` insertion order; then
2. the session transport's `dispose` error.

The loss occurs only at current lines 1364-1369:

```ts
entry.lifecycle.lastError = errors
	.map((error) => error.message)
	.join("; ");
throw new Error(entry.lifecycle.lastError);
```

`toError` at current lines 1585-1587 returns an existing `Error` unchanged, so
the collected array still contains the exact `SidecarRequestRejected` or other
typed errors. The final `new Error` is the destructive step.

### Whole-client cleanup discards per-session structure

`AgentOsSidecarClient.dispose` at current lines 1381-1391 tries every session
and collects each rejection. It then creates another joined-message `Error`.
After fixing `disposeSession`, each child may itself be an `AggregateError` with
the failed session's context. Keep that nesting. Recursive flattening would
discard which session transaction produced a cause and adds unnecessary client
logic; nested `.errors` still makes every original typed leaf inspectable.

### Shared-sidecar test cleanup discards sidecar errors

`__disposeAllSharedSidecarsForTesting` clears the process-global cache before it
attempts every disposal, which is appropriate for a test-only drain. Its final
plain `Error` at current lines 3548-3551 loses the returned per-sidecar errors.
The helper should keep its current try-all order and cache behavior and only
change the thrown error representation.

## Exact production replacements

### `packages/core/src/sidecar/rpc-client.ts`

Replace the `disposeSession` failure block with:

```ts
if (errors.length > 0) {
	entry.lifecycle.state = "failed";
	entry.lifecycle.lastError = errors
		.map((error) => error.message)
		.join("; ");
	throw new AggregateError(
		errors,
		`failed to dispose sidecar session ${sessionId}: ${entry.lifecycle.lastError}`,
	);
}
```

The detail suffix intentionally preserves the existing
`rejects.toThrow("session close failed")` behavior and keeps lifecycle listings
useful. The `errors` array is now the authoritative structured representation.

Replace the client-level failure block with:

```ts
if (errors.length > 0) {
	throw new AggregateError(
		errors,
		`failed to dispose sidecar client: ${errors
			.map((error) => error.message)
			.join("; ")}`,
	);
}
```

Do not add a custom cleanup error class, a recursive flatten helper, a
sidecar-error-code parser, or a client retry policy. Existing state transitions
already make failed session/client cleanup retryable.

### `packages/core/src/agent-os.ts`

Replace only the final throw in `__disposeAllSharedSidecarsForTesting`:

```ts
if (errors.length > 0) {
	throw new AggregateError(
		errors,
		`failed to dispose shared sidecars: ${errors
			.map((error) => error.message)
			.join("; ")}`,
	);
}
```

Do not alter `AgentOsSidecar.disposeOnce` or
`createInProcessSidecarTransport.disposeTransport`; both already implement the
desired behavior. Do not add protocol fields or a sidecar command for host
error aggregation.

### `crates/client/src/sidecar.rs`

After the existing shared-pool removal block in `AgentOsSidecar::dispose`,
replace the always-empty aggregation scaffold with an unconditional success:

```rust
Ok(())
```

Concretely, delete `let errors: Vec<String> = Vec::new();` and the final
`if errors.is_empty() { ... } else { ... }` block. This is dead-code removal,
not new Rust behavior: the method currently has no operation that can populate
that vector, while real VM/session disposal errors are returned from
`AgentOs::shutdown` before the lease is released.

### `website/src/content/docs/docs/core.mdx`

After the explicit-sidecar `<CodeSnippet>` at current line 46, add:

```mdx
Sidecar teardown attempts every sibling cleanup operation it owns. If any
cleanup fails, it rejects with an `AggregateError` whose `errors` array keeps the
original typed failures in cleanup order.
```

This documents a public error contract without exposing internal wrapper depth
or moving policy into the client.

## Before and after tests

Use real `SidecarRequestRejected` instances so the regression proves more than
message retention. A small test helper can construct one with a real response
frame using `SidecarRequestRejected` from
`@rivet-dev/agentos-runtime-core/sidecar-errors` and
`SIDECAR_PROTOCOL_SCHEMA` from
`@rivet-dev/agentos-runtime-core/protocol-schema`:

```ts
function rejectedError(code: string, message: string, requestId: number) {
	const ownership = {
		scope: "session" as const,
		connection_id: "connection-1",
		session_id: "session-1",
	};
	return new SidecarRequestRejected({
		code,
		message,
		response: {
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: requestId,
			ownership,
			payload: { type: "rejected", code, message },
		},
	});
}
```

### Focused client unit tests

Extend `packages/core/tests/sidecar-client.test.ts` with two tests.

1. `preserves VM and transport failures when session cleanup fails`

   - create one session and one VM through the existing injected transport;
   - make `disposeVm` reject with one `SidecarRequestRejected` and transport
     `dispose` reject with another;
   - await `session.dispose()` and capture the error;
   - write the final structured assertions first; running them against the old
     production code must fail because the result is a plain `Error` with only
     joined messages and exposes neither source object;
   - assert it is `AggregateError`, its contextual message
     includes the session ID, and `error.errors` is exactly
     `[vmError, transportError]` by object identity and order; and
   - make the injected operations succeed on retry and retain the existing
     lifecycle assertion that the session becomes `disposed`.

2. `preserves every failed session when client cleanup fails`

   - create two sessions whose transport `dispose` methods reject with distinct
     structured errors;
   - assert both disposals were attempted;
   - run the final test before the production edit and record its failure on the
     single plain joined-message error;
   - assert the client-level error is `AggregateError` with two
     per-session `AggregateError` children in session insertion order, and each
     nested `.errors` array contains its exact original rejection; and
   - retry successfully so `AgentOsSidecarClient.disposed` behavior and session
     lifecycle do not regress.

Do not weaken the existing test at current lines 136-158,
`retries client disposal after a session transport failure`. Its message and
retry expectations should continue to pass unchanged.

Use identity assertions, not deep equality, for the leaves:

```ts
expect(error).toBeInstanceOf(AggregateError);
const aggregate = error as AggregateError;
expect(aggregate.errors).toHaveLength(2);
expect(aggregate.errors[0]).toBe(vmError);
expect(aggregate.errors[1]).toBe(transportError);
```

### Shared-sidecar cleanup unit test

Add `packages/core/tests/shared-sidecar-cleanup-errors.test.ts`:

- call `__disposeAllSharedSidecarsForTesting()` before arranging the test so
  unrelated cached handles cannot affect insertion order;
- acquire two unused handles with unique pools through
  `AgentOs.getSharedSidecar` (unused handles spawn no native process);
- use `vi.spyOn(handle, "dispose").mockRejectedValue(...)` to inject two real
  structured errors;
- call `__disposeAllSharedSidecarsForTesting`;
- assert both spies ran once even though the first rejected;
- run this desired assertion before the production edit and record that it
  receives a plain joined-message error; and
- assert `AggregateError.errors` contains the two injected
  objects in pool insertion order and the message retains the shared-sidecar
  context.

The helper clears `sharedSidecars` before disposal. In `finally`, restore both
spies and call both real `dispose()` methods with `Promise.allSettled`; no native
process was spawned, but this leaves both detached handles terminal rather than
merely relying on garbage collection.

### Public lease/injected-transport regression

Add one focused case to `packages/core/tests/sidecar-placement.test.ts` named
`preserves a typed VM-admin failure through lease cleanup`:

1. create an explicit sidecar and one `AgentOs` VM with
   `defaultSoftware: false`;
2. obtain the test-visible `_sidecarLease.admin` through the same narrow
   structural cast style already used by lifecycle tests;
3. temporarily make `admin.dispose()` reject with a real
   `SidecarRequestRejected`;
4. call `sidecar.dispose()` and recursively inspect `AggregateError.errors`;
5. before the remaining edit, the source rejection disappears at
   `AgentOsSidecarClient.disposeSession`; after it, the exact source object is a
   nested leaf through the already-compliant injected transport, session
   client, lease, and sidecar aggregates; and
6. restore `admin.dispose`, then dispose the VM and sidecar successfully to
   prove all retained cleanup state is retryable.

This is the regression that closes the tracker's lease and injected-transport
language without exposing a new test-only production API or rewriting code that
is already correct. Assert the exact source object is found recursively; do not
assert a brittle number of wrapper levels.

Use a small recursive helper whose base case is object identity:

```ts
function containsError(error: unknown, target: Error): boolean {
	if (error === target) return true;
	return (
		error instanceof AggregateError &&
		error.errors.some((child) => containsError(child, target))
	);
}
```

Also strengthen
`packages/core/tests/leak-rpc-client.test.ts:187-225` by retaining the injected
`disposeVm` error object and asserting the first rejected `proxy.dispose()` is
an `AggregateError` whose first child is that exact object. This guards an
already-compliant adjacent path and proves its retry behavior did not regress.
This assertion should already pass before the production edit; record it as the
before/after guard, not as one of the red regressions.

### Focused validation

Add the four desired structured-error regressions before changing production
code, then run the first command and save the failures as the tracker's
"validated before" evidence. The session, whole-client, shared-sidecar, and
public lease-chain tests must fail on the old plain-error sites; the strengthened
proxy identity/retry assertion must already pass. After applying the production
edit, rerun the complete list:

```sh
pnpm --dir packages/core exec vitest run tests/sidecar-client.test.ts tests/shared-sidecar-cleanup-errors.test.ts tests/sidecar-placement.test.ts tests/leak-rpc-client.test.ts --reporter=verbose
pnpm --dir packages/core check-types
cargo fmt --all -- --check
cargo test -p agentos-client sidecar::tests
pnpm --dir website build
git diff --check
```

The current pre-edit baseline for the three existing TypeScript files is 11/11
passing, and the Rust sidecar unit filter is 8/8 passing:

```sh
pnpm --dir packages/core exec vitest run tests/sidecar-client.test.ts tests/sidecar-placement.test.ts tests/leak-rpc-client.test.ts --reporter=verbose
cargo test -p agentos-client sidecar::tests
```

No protocol fixture regeneration, native-sidecar test, or browser-adapter test
is required. The Rust unit command is sufficient because its edit only removes
an unreachable branch, and the website build covers the one documentation
sentence.

## Risks and dependencies

- **Preserve retry semantics.** Do not set session/client/sidecar state to
  disposed until every current cleanup operation succeeds. This item changes
  only the error object returned from failed attempts.
- **Preserve exact `Error` identity.** `toError` already does this for real
  errors. Continue normalizing non-`Error` throws, but never clone or reconstruct
  `SidecarRequestRejected` children.
- **Keep deterministic order.** VM/session/sidecar `Map` insertion order is the
  observable `.errors` order and should match cleanup attempt order.
- **Keep nested transaction context.** A client-level aggregate should contain
  per-session aggregates. Flattening them into one leaf list would make the
  client more complicated and discard useful transaction boundaries.
- **Joined messages are not themselves the bug.** They are acceptable display
  text when the original child objects are also retained. Removing child
  messages from `AggregateError.message` would unnecessarily break existing
  `.toThrow(message)` coverage and degrade logs that do not print `.errors`.
- **Node support is sufficient.** The package requires Node 20 and already uses
  `AggregateError` in the same source files; no polyfill or custom class is
  needed.
- **Items 59 and 60 may also create `AggregateError`.** Reuse this ordering rule:
  primary operation error first, cleanup error second. They do not block Item
  65 and should stay in their own revisions.
- **Item 63 is complementary.** Once process-terminal and ACP errors become
  exported structured classes, these cleanup aggregates will preserve them
  automatically. Item 65 must not add knowledge of those classes or codes.
- **Warning-only startup cleanup is separate.** `AgentOs.create` and failed lease
  creation currently log some secondary cleanup failures while returning the
  primary error. That deserves its own fail/retry analysis because it is not a
  multi-error string flattener and can affect resource ownership. Do not expand
  this small representation fix without a dedicated tracker item.
- **Rust parity is semantic, not type-name parity.** JavaScript has a standard
  `AggregateError`; Rust does not need a new public enum variant when it performs
  only one authoritative close operation. Retain/downcast the existing typed
  `ClientError` instead of manufacturing a message-only aggregate.

## Dedicated `jj` revision boundary

Use one dedicated stacked revision, for example
`fix(client): preserve structured cleanup errors`, containing only:

- `packages/core/src/sidecar/rpc-client.ts`;
- the three-line error-representation change in
  `packages/core/src/agent-os.ts`;
- `packages/core/tests/sidecar-client.test.ts`;
- `packages/core/tests/shared-sidecar-cleanup-errors.test.ts`;
- the focused public-chain regression in
  `packages/core/tests/sidecar-placement.test.ts`;
- the identity assertion in
  `packages/core/tests/leak-rpc-client.test.ts`;
- dead parity-scaffold removal in `crates/client/src/sidecar.rs`;
- the disposal-contract sentence in
  `website/src/content/docs/docs/core.mdx`; and
- the Item 65 checklist/status update in
  `docs/thin-client-migration.md` after all focused tests pass.

Do not include lease-state refactors, startup cleanup redesign, sidecar/protocol
changes, other Rust changes, custom error types, or work from Items 59, 60, or
63. Verify the shared working-copy diff before describing and stacking the
revision.

The final tracker evidence should name all four failing-before regression tests
(session, client, shared handles, and public lease chain), the already-passing
proxy identity/retry guard, the successful after commands, and the dedicated
`jj` revision ID.
