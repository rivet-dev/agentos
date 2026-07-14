# Item 62 research: put toolkit permission assertions at the sidecar boundary

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 62 tracker status.

Inspected on **2026-07-14** at revision **`bd5dca291388`**. Tracker anchors are
`docs/thin-client-migration.md:108` (issue inventory), current line 189
(pending status), and current line 275 (before/after/complete checklist).

## Recommendation

Correct the three stale assertions in
`packages/core/tests/toolkit-permissions.test.ts` without adding any client
permission logic:

1. change the partial-policy test with omitted `binding` from expected denial to
   expected success;
2. delete the direct captured-handler test that expects explicit `binding: deny`
   to be enforced by the TypeScript callback; and
3. delete the direct captured-handler test that expects a non-matching binding
   pattern to be enforced by that callback.

Move the two explicit-deny assertions, and an exact partial-policy omission
assertion, into the existing native-sidecar tool-command integration coverage in
`crates/native-sidecar/tests/service.rs`. Those tests must drive
`agentos-<toolkit>` through sidecar command resolution and prove a denied command
never emits the sidecar-to-host callback. Keep TypeScript's captured-handler
tests only for behavior that actually belongs on the host: complete Zod parsing,
transforms/refinements, hostile-field stripping, callback execution/result
mapping, unknown tools, and legacy-shape rejection.

No production change is needed. In particular, do not restore a permission map
or permission evaluator to `handleHostCallback`.

Priority: **P2**. Confidence: **high**. The current focused suite reproduces all
three stale expectations, the sidecar enforcement point is explicit, and the
native sidecar already has adjacent allow/deny tool-command tests that need only
small extensions.

## Cross-layer disposition

| Layer | Exact current code | Item 62 disposition |
|---|---|---|
| TypeScript permission forwarding | `packages/core/src/sidecar/permissions.ts:29-65`, runtime conversion at `packages/runtime-core/src/sidecar-process.ts:2040-2053`, and omission tests at `packages/core/tests/sidecar-permission-descriptors.test.ts:101-178` | **No production change.** Omitted top-level policy and omitted domains already remain omitted on the wire. Keep these forwarding tests. |
| TypeScript host callback | `packages/core/src/agent-os.ts:1013-1071` and registration routing at `:2816-2859`; direct-handler fixture at `packages/core/tests/toolkit-permissions.test.ts:7-94` | **Test cleanup only.** The handler owns complete Zod parsing and execution after authorization; it intentionally has no permission context. Remove policy assertions that bypass the sidecar. |
| Protocol | optional domains in `crates/sidecar-protocol/protocol/agentos_sidecar_v1.bare:158-165`, host callback registration at `:264-292`, and the sidecar-to-host callback exchange at `:1006-1048` | **No change.** Existing optional fields preserve omission and the request direction already represents authorized sidecar dispatch. |
| Shared sidecar defaults | `permissions_with_allow_all_defaults` in `crates/native-sidecar-core/src/permissions.rs:57-73`; native application at `crates/native-sidecar/src/vm.rs:178-200`; browser application at `crates/native-sidecar-browser/src/wire_dispatch.rs:1604-1608` | **No production change.** Public omission is already normalized once to allow-all. Keep the raw evaluator's missing-scope fail-closed behavior. |
| Native enforcement | `crates/native-sidecar/src/tools.rs:252-307`, with adjacent service tests at `crates/native-sidecar/tests/service.rs:10530-10685` and reusable callback counters at `:10743-10793,10842-10902` | **Strengthen integration tests.** Prove explicit deny and pattern mismatch emit zero host callbacks; prove partial omission allows one callback. |
| Browser sidecar | shared normalization above; registration/initialization in `crates/native-sidecar-browser/src/wire_dispatch.rs:868-900,1604-1777` and reverse-host-callback rejection coverage in `crates/native-sidecar-browser/tests/wire_dispatch.rs:3579` | **No change.** Item 62 concerns native `agentos-<toolkit>` command enforcement; browser default normalization already uses the shared implementation. |
| Rust SDK/actor | Rust transports typed callback frames but does not execute TypeScript `HostTool` closures; actor/native calls ultimately use the same sidecar resolver | **No change or new Rust client test.** Adding Rust or actor policy evaluation would duplicate the enforcement point. |
| Docs/tracker | tracker rows above; Items 3, 9, 25, 26, and 61 document the established boundaries | **Evidence/status only after tests pass.** No website or public API behavior changes. |

This is deliberately a test migration. The smallest compliant implementation
changes no production code: it makes client tests assert only forwarding and
host-owned Zod behavior, while security assertions execute the guest command at
the sidecar enforcement boundary.

## Correct boundary

```text
untrusted guest command/input
  -> native sidecar resolves agentos-<toolkit>
  -> native sidecar evaluates binding.invoke for <toolkit>:<tool>
       deny -> exit 1; no host callback is emitted
       allow -> emit typed host_callback request
  -> TypeScript validates with the complete Zod schema exactly once
  -> TypeScript executes the registered host callback
```

The client can validate and execute the callback only after the trusted sidecar
has authorized dispatch. Calling the captured TypeScript handler directly in a
unit test deliberately skips the first three steps; such a call cannot prove or
disprove guest permission enforcement.

## Original issue and observed before behavior

The tracker identifies three cases at
`docs/thin-client-migration.md:108,189,275`. All three fail against the current
sidecar-owned model.

Running:

```sh
pnpm --dir packages/core exec vitest run \
  tests/toolkit-permissions.test.ts --fileParallelism=false
```

currently produces these Item 62 failures:

| Current test | Current expectation | Actual correct behavior |
|---|---|---|
| `denies toolkit invocation by default until tool permissions are granted` | A partial policy containing `fs` and `childProcess` but omitting `binding` denies `math:add`. | Exit code is 0 and the result is `{ sum: 12 }`; omitted domains inherit sidecar allow-all. |
| `denies host_callback RPC tool invocation when binding.invoke policy is deny (not just the CLI path)` | Calling the captured TypeScript callback handler directly consults `binding: deny`. | The handler performs Zod parse and calls `execute`; sidecar authorization was bypassed by the test. |
| `host_callback RPC respects binding.invoke pattern scope and denies a non-matching tool` | Calling the captured TypeScript callback handler directly rejects `math:danger`. | The handler performs Zod parse and calls `danger`; pattern enforcement occurs before the sidecar emits this callback. |

The same run has a fourth, independent stale assertion:
`rejects duplicate toolkit registration with a conflict` expects the message to
be prefixed with `conflict:`. The sidecar now returns typed code `conflict` and
the unmodified message `toolkit already registered: math`, so this full-file
gate cannot pass until that expectation is corrected too. Treat that as a tiny
Item 26 follow-through in the same test-only revision; it is not a fourth
permission-model case.

Six remaining tests pass, including real default-allow and matching-rule command
invocations plus all four host-side callback/Zod cases.

## Exact current code paths

### TypeScript forwards omission

`serializePermissionsForSidecar` at
`packages/core/src/sidecar/permissions.ts:29-65` copies explicit scopes and
leaves every omitted domain `undefined`. Its focused tests at
`packages/core/tests/sidecar-permission-descriptors.test.ts:5-178` already prove
complete and partial omission is preserved. Item 62 should not change this
serializer.

`AgentOs.create` builds the sidecar VM request at
`packages/core/src/agent-os.ts:1475-1502`; tool definitions are forwarded as
`hostCallbacks`, while permission defaults are not selected there.

### The sidecar normalizes and enforces

`permissions_with_allow_all_defaults` at
`crates/native-sidecar-core/src/permissions.rs:57-73` resolves both a wholly
omitted policy and omitted domains in a partial policy to allow-all. Native VM
creation applies it at `crates/native-sidecar/src/vm.rs:175-197`, before storing
the effective configuration.

Tool command resolution performs the authoritative check in
`crates/native-sidecar/src/tools.rs:252-307`:

```rust
evaluate_permissions_policy(
    &vm.configuration.permissions,
    "binding",
    "binding.invoke",
    Some(&callback_key),
)
```

Any result other than `Allow` returns
`blocked by binding.invoke policy for {callback_key}` before
`ToolCommandResolution::Invoke` and its `HostCallbackRequest` are constructed.
This ordering is the security property the moved tests must exercise.

The adjacent native service tests at
`crates/native-sidecar/tests/service.rs:10530-10685` already cover:

- an explicit all-binding deny causing command exit 1; and
- a matching `math:add` allow rule emitting one callback and returning success.

They do not yet prove partial-policy omitted binding allows the callback, nor
that a non-matching rule denies without dispatch. The explicit-deny test also
does not install a counting callback handler, so its current result implies but
does not directly assert that no host callback was emitted.

### The TypeScript callback intentionally contains no policy

`handleHostCallback` at `packages/core/src/agent-os.ts:1014-1058` does exactly
the remaining host work:

1. require a typed `host_callback` payload;
2. find the registered tool;
3. run the tool's complete Zod schema; and
4. execute it or return its host error.

Its context at lines 1060-1072 contains only a tool map. It has no permissions,
and `_installSidecarRequestHandler` at lines 2816-2859 routes a sidecar callback
straight to it. This is correct. Reintroducing client permission state here
would duplicate the sidecar decision and recreate Item 3.

`createVmCapturingHandler` in the test at current lines 23-56 spies on
`registerSidecarRequestHandler` and returns that handler as an ordinary
function. `created.handler(hostCallbackFrame(...))` therefore does not send a
guest request through the sidecar; it invokes the post-authorization host route
directly. The comments at current lines 7-21 incorrectly describe that as “the
bytes an untrusted guest controls” and must be rewritten.

## Exact test edits

### `packages/core/tests/toolkit-permissions.test.ts`

#### Preserve the real command tests but correct omission

Rename the current partial-policy case at lines 181-202 to approximately:

```text
allows toolkit invocation when binding is omitted from a partial policy
```

Keep this input unchanged:

```ts
permissions: {
	fs: "allow",
	childProcess: "allow",
},
```

The omission itself is the subject. Change the assertions to exit code 0,
empty stderr, and the normal JSON result `{ ok: true, result: { sum: 12 } }`.
Do not add `binding: "allow"`; doing so would stop testing sidecar-owned partial
defaults.

Keep the existing wholly omitted default-allow test and matching explicit-rule
test. Together they remain a thin TypeScript end-to-end check that the client
forwards toolkit definitions and explicit/omitted policy input without
interpreting it.

#### Remove the two direct policy tests

Delete the tests currently at lines 247-343:

- `denies host_callback RPC tool invocation when binding.invoke policy is deny
  (not just the CLI path)`; and
- `host_callback RPC respects binding.invoke pattern scope and denies a
  non-matching tool`.

Do not invert them into tests saying a denied callback “is allowed” when the
handler is called directly. That would document an impossible production route
and look like a security bypass. Their policy assertions move to the native
sidecar service test described below.

#### Reframe the remaining captured-handler tests

Replace the file header and the second `describe` title. State that the spy
captures the trusted sidecar-to-client callback route after authorization and
that synthetic invocation is only for host parsing/execution tests. A suitable
title is:

```ts
describe("toolkit host callback validation", () => { ... });
```

Retain these tests:

- hostile/extra input is stripped and cannot pollute prototypes;
- a transform/refinement is applied exactly once;
- invalid Zod input never reaches `execute`; and
- a legacy command-shaped payload is treated only as tool input and rejected by
  Zod, rather than reviving a client command dispatcher.

For the hostile-input, invalid-input, and legacy-shape tests, remove the
`permissions` objects from `createVmCapturingHandler`. Those explicit
allow/deny settings are irrelevant after the test calls the handler directly
and currently imply policy is involved when it is not. Keep only
`{ toolKits: [kit] }`.

Revise the security comments carefully: callback `input` is still untrusted and
must receive the complete host Zod parse, but the callback frame itself is
sidecar-originated after authorization. Do not weaken hostile-input or
single-parse assertions.

Item 61 replaces the transform test's monkey-patched `safeParse` with a real
transform/refinement schema. Stack Item 62 after Item 61 and preserve that real
schema rather than restoring the current workaround.

#### Correct the unrelated typed conflict assertion

Import the already-exported `SidecarRequestRejected` from the Core root and
replace the obsolete message-prefix regex with an assertion on structured
fields:

```ts
const creation = AgentOs.create({
	toolKits: [mathToolKit, duplicateMathToolKit],
});
await expect(creation).rejects.toBeInstanceOf(SidecarRequestRejected);
await expect(creation).rejects.toMatchObject({
	code: "conflict",
	message: "toolkit already registered: math",
});
```

If Vitest's repeated `.rejects` handling is unclear for the same promise, use a
single `try/catch` and assert both the class and fields on the caught value. Do
not put `conflict:` back into the message; code and message are distinct sidecar
fields after Item 26.

### `crates/native-sidecar/tests/service.rs`

Keep all policy tests inside the existing included `service::tests` module and
the `run_service_suite` execution model; the file deliberately consolidates
V8-backed service work to avoid teardown/init crashes across many libtest cases.

#### Strengthen explicit all-binding deny

Rename
`tools_javascript_child_process_denies_host_callback_without_permission` to
approximately
`tools_javascript_child_process_explicit_binding_deny_skips_host_callback`.
The configuration already supplies
`binding: Some(PermissionMode::Deny)` and should remain explicit.

Before spawning `agentos-math add`, install a handler backed by
`Arc<AtomicUsize>` that increments if it receives a `HostCallback`. After the
existing exit-code/stderr assertions, assert the count is zero. Return an error
from the unexpected handler rather than relying only on the absence of a
configured handler. This proves authorization happens before host dispatch.
Reuse the exact counter/handler shape already present in this file at current
lines 10743-10753 and 10842-10861; no new test utility or dependency is needed.

#### Add partial-policy omitted binding allow

Add
`tools_javascript_child_process_omitted_binding_defaults_to_allow` beside the
existing deny/allow cases:

1. create the VM with `fs: allow`, `child_process: allow`, and `binding: None`;
2. register `math:add`;
3. install a counting host-callback handler returning a known result;
4. spawn `/usr/local/bin/agentos-math add` through
   `spawn_javascript_child_process_sync`; and
5. assert exit 0, the known JSON result, and invocation count 1.

Use a partial `PermissionsPolicy`, not `PermissionsPolicy::allow_all()`. The
test must exercise VM normalization of an omitted domain.

#### Add non-matching pattern deny

Add
`tools_javascript_child_process_non_matching_binding_pattern_skips_host_callback`:

1. create a binding rule set with default deny and one allow rule for operation
   `invoke`, pattern `math:safe`;
2. register `math:danger`;
3. install a counting callback handler;
4. invoke `agentos-math danger`; and
5. assert exit 1, empty stdout, stderr contains
   `blocked by binding.invoke policy for math:danger`, and callback count 0.

The existing matching-rule test at current lines 10594-10685 already proves the
positive side of the same pattern evaluator. Keep it and, if helpful, rename it
to make the pairing obvious.

Add the renamed and new functions to `run_service_suite` at current lines
21384-21390. Do not create a second client-side permission fixture or duplicate
the resolver in test code.

## Before and after checklist

### Before behavior

- [ ] The full TypeScript toolkit suite records the three Item 62 failures:
  partial omitted binding unexpectedly succeeds, and both direct-handler policy
  tests unexpectedly execute their callback.
- [ ] The same baseline records the independent stale `conflict:` message
  assertion so the after gate is not misreported as an Item 62 regression.
- [ ] Source inventory confirms `handleHostCallback` has no permission context
  and the sidecar checks `binding.invoke` before constructing the callback.

Research-time baseline evidence at `bd5dca291388`:

| Command | Result |
|---|---|
| `pnpm --dir packages/core exec vitest run tests/toolkit-permissions.test.ts --fileParallelism=false` | **Expected vulnerable baseline: 4 failed, 6 passed.** Three failures are Item 62 (partial omission and two direct-handler policy claims); the fourth is the stale Item 26 conflict-message assertion. |
| `pnpm --dir packages/core exec vitest run tests/sidecar-permission-descriptors.test.ts --fileParallelism=false` | **Pass: 4 passed.** Omission/partial forwarding is already correct. |
| `pnpm --dir packages/core check-types` | **Pass.** |
| `cargo test -p agentos-native-sidecar-core permissions --lib` | **Pass: 9 passed.** Raw evaluator coverage remains fail-closed; public allow-all comes from VM normalization. |

### After behavior

- [ ] Wholly omitted and partial-policy omitted binding both allow real toolkit
  command invocation through the sidecar.
- [ ] Explicit all-binding deny exits 1 and invokes the sidecar-to-host callback
  zero times.
- [ ] A non-matching binding pattern exits 1 and invokes the callback zero times;
  the existing matching pattern invokes it once.
- [ ] Direct TypeScript callback tests contain no binding-policy expectation or
  irrelevant permissions setup; they cover only complete Zod parse and host
  callback behavior.
- [ ] The duplicate registration test asserts typed code and unmodified message
  separately.
- [ ] No production TypeScript, Rust client, protocol, or sidecar implementation
  changes are present.
- [ ] Item 62 is marked `done` only after the tracker records before/after test
  evidence.

Focused validation commands:

```sh
pnpm --dir packages/core exec vitest run \
  tests/toolkit-permissions.test.ts --fileParallelism=false
pnpm --dir packages/core exec vitest run \
  tests/sidecar-permission-descriptors.test.ts --fileParallelism=false
pnpm --dir packages/core check-types
cargo test -p agentos-native-sidecar-core permissions --lib
```

The native service integration is an explicit expensive gate because the
current harness intentionally runs its service cases through one top-level
suite:

```sh
cargo test -p agentos-native-sidecar --test service \
  aad_javascript_network_dns_javascript_net_poll_suite -- --test-threads=1
cargo check --workspace
git diff --check
```

## Client-to-sidecar test migration

This item is a test-ownership migration, not a production migration.

Move these claims to native sidecar integration coverage:

- explicit binding deny blocks `math:add` before callback dispatch;
- a non-matching allow pattern blocks `math:danger` before dispatch; and
- partial-policy omission resolves to allow before tool authorization.

Keep these claims in TypeScript:

- omitted/explicit permission objects are serialized without client defaults;
- a real toolkit command composes with the sidecar end to end;
- the complete Zod schema validates hostile/refined/transformed input once; and
- only validated data reaches the host callback.

No Rust SDK test is needed. The Rust SDK does not execute TypeScript `HostTool`
closures, and adding a Rust client permission check would violate the same
boundary. No browser-sidecar tool invocation test is required for this item:
browser permission-default normalization is already shared/covered, while the
authoritative `agentos-<toolkit>` command resolver under test is the native
sidecar implementation.

## Dependencies and overlap

- **Item 3 is foundational and already done.** It established omitted/partial
  allow-all normalization in native/browser sidecars and removed client
  re-enforcement. Item 62 aligns stale tests with that contract; it must not
  reopen the policy decision.
- **Items 9 and 25 define tool ownership.** The sidecar owns command parsing,
  binding policy, and dispatch; TypeScript owns the one complete Zod parse and
  callback execution.
- **Item 61 should be the direct parent.** It edits the transform test in this
  same file to use a real refinement/transform. Preserve that work while
  removing only permission assertions and irrelevant permission setup.
- **Item 26 explains the fourth baseline failure.** Normal rejections expose
  typed code separately from the exact sidecar message. The small conflict-test
  update is needed for the Item 62 full-file gate but requires no production
  change.
- **Item 67 is adjacent.** It changes cleanup/delivery when a host permission
  callback handler throws, not tool `binding.invoke` enforcement. Do not add
  permission policy to the host callback route while preserving its error
  handling changes.
- **Item 38 is documentation-only.** Its omitted-permission claim verifier can
  run independently; this revision should not edit website/security prose.

## Risks and review points

- **Do not make the direct callback test a security boundary.** It is a captured
  post-authorization function, not a guest-to-sidecar request path.
- **Prove absence of dispatch.** Exit code and stderr alone are weaker than a
  callback counter; explicit deny and pattern mismatch must assert zero host
  invocations.
- **Do not add `binding: "allow"` to the omission case.** That would erase the
  contract being tested.
- **Do not change raw evaluator fail-closed behavior.**
  `evaluate_permissions_policy` returns deny for a missing scope as an internal
  safeguard. VM creation/configuration must normalize public omission to a
  complete allow-all-effective policy before evaluation. Item 62 tests that
  pipeline; it should not make arbitrary unnormalized policies permissive.
- **Keep Zod authoritative on the host.** Sidecar JSON Schema parsing does not
  replace TypeScript refinements/transforms or hostile-input stripping.
- **Preserve exact callback key matching.** Tests should use `binding.invoke`
  with `math:add`, `math:safe`, and `math:danger`; do not weaken them to a broad
  wildcard that fails to prove pattern behavior.
- **Keep the native service harness stable.** Add functions to the consolidated
  suite rather than creating parallel V8-heavy top-level tests.

## Bounded dedicated JJ revision

Create one new stacked JJ revision for Item 62, after Item 61, and keep it to:

```text
packages/core/tests/toolkit-permissions.test.ts
crates/native-sidecar/tests/service.rs
docs/thin-client-migration.md  # evidence/status only, last
```

No production TypeScript/Rust source, Rust client test, protocol schema,
generated codec, browser sidecar, package manifest, dependency, lockfile,
website, or secure-exec mirror edit is expected. If implementation requires one,
stop and identify a separate behavior bug instead of hiding it inside this
test-alignment revision.
