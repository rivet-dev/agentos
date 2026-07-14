# Item 38 research — permission-default documentation

Status: implementation-ready research only. This note does not modify production
code, tests, or the Item 38 tracker status.

- **Priority:** P1. Active security guidance states the opposite of the
  sidecar-enforced product behavior, so callers can accidentally run untrusted
  code with broader authority than the documentation promises.
- **Fix confidence:** High. Native and browser creation both call the same
  normalization helper, and focused tests already lock in omission as
  allow-all.
- **Implementation dependency:** Item 3 is the completed semantic prerequisite.
  Item 38 must be its own stacked revision after Item 37. Items 51 and 55 extend
  the verifier introduced here; Item 62 separately owns stale TypeScript
  permission-enforcement tests and must not be folded into this docs revision.
  Item 39 is the next stack child and also edits `README.md`, but owns only the
  quickstart; Item 38 owns the two security bullets and must leave the quickstart
  untouched.

## Finding

The public AgentOS product default is unambiguously **allow-all**, but several
active documentation surfaces still advertise a mixed or deny-by-default policy.
The implementation and native/browser tests already agree, so Item 38 should be
a documentation and CI claim-verifier revision, not a runtime change.

The exact runtime behavior is:

1. `permissions: None` is normalized to all six scopes set to `allow` by
   `permissions_with_allow_all_defaults` in
   `crates/native-sidecar-core/src/permissions.rs:57-74`.
2. An explicit partial top-level policy also inherits `allow` for every omitted
   scope. For example, `{ fs: "deny" }` still has `network`, `childProcess`,
   `process`, `env`, and `binding` set to `allow`.
3. Inside a scope that is explicitly represented as a rule set, an omitted
   rule-set `default` is **deny** (`unwrap_or(PermissionMode::Deny)` at
   `crates/native-sidecar-core/src/permissions.rs:125` and line 145). Last
   matching rule wins.
4. The generic kernel remains fail-closed when handed an unnormalized policy
   with missing domains. AgentOS callers do not receive that generic default:
   native create normalizes in
   `crates/native-sidecar/src/vm.rs::NativeSidecar::create_vm` immediately after
   decoding `CreateVmConfig` (currently line 197), native configure normalizes
   explicit replacements in `NativeSidecar::configure_vm` (currently lines
   450-456), and browser create normalizes in
   `crates/native-sidecar-browser/src/wire_dispatch.rs::BrowserWireDispatcher::create_vm`
   (currently line 1572). Browser configure uses the same helper for explicit
   replacement policies at lines 724-735 and preserves omission as "leave the
   installed policy unchanged."
5. Allowing a permission scope does not manufacture a host capability. The guest
   still sees only its virtual process/filesystem/network paths and only mounts
   or bindings that the trusted host explicitly configured. Network permission
   is also separate from listener loopback confinement and destination/egress
   controls.

Authoritative existing behavior tests include:

- `crates/native-sidecar/src/vm.rs` — omitted domains in a partial policy inherit
  the sidecar allow-all default. The exact anchor is
  `vm::tests::omitted_permission_domains_use_the_sidecar_allow_all_default`
  (currently line 2330).
- `crates/native-sidecar/tests/service.rs:8102` — a VM with no policy can read and
  write as a guest in
  `create_vm_without_permissions_defaults_to_static_allow_all`.
- `crates/native-sidecar-browser/tests/wire_dispatch.rs::browser_wire_create_vm_without_permissions_defaults_to_allow_all`
  (currently line 2279) — browser wire create with no policy defaults to
  allow-all.
- `packages/runtime-browser/tests/runtime/converged-permissions.test.ts:5` — the
  converged browser harness uses allow-all.

Do not change or delete generic-kernel deny tests such as
`crates/kernel/tests/default_deny_guards.rs` or
`crates/native-sidecar-core/src/permissions.rs:407`. They validate the lower
layer's fail-closed behavior before the AgentOS sidecar applies its product
default and are not contradictory.

## Incorrect public claims

The following active claims must change in both website source MDX and the
checked public Markdown copy where both exist.

### Root README

- `README.md:18` says filesystem, network, and process permissions are
  deny-by-default.
- `README.md:128` links to “Deny-by-default permissions.”

Replace these with sidecar-enforced/granular permission language that states
plainly that omitted permissions are allow-all and an explicit policy is needed
to restrict untrusted workloads. Do not retain “deny-by-default” as marketing
shorthand.

### Permissions page

- `website/src/content/docs/docs/permissions.mdx:12,18-32,38-47,57` and
  `website/public/docs/docs/permissions.md:10,16-30,34-43,51` describe a mixed
  “secure baseline” with network and binding denied.

Rewrite the defaults section around these exact rules:

```text
Omitting `permissions` selects the sidecar-owned allow-all product default.
All six scopes are `allow`: fs, network, childProcess, process, env, binding.
An explicit partial top-level policy also leaves omitted scopes at `allow`.
Within an explicit rule-set scope, omitting that rule set's `default` means
`deny`; specify `default: "allow"` for a deny-list or `default: "deny"` for an
allow-list.
```

The scope table must show `allow` for every row, including network and binding,
and the obsolete binding auto-grant footnote should be removed. Add a warning
that allow-all authorizes only capabilities present inside/configured for the VM;
it does not expose the real host filesystem or host process table.

The `grant-network` snippet at
`website/src/content/docs/docs/permissions.mdx:34` is now a no-op because network
is already allowed on omission. Remove it rather than teaching a redundant
override. Correspondingly simplify:

- `examples/permissions/server.ts:4-7,37-44`: remove `grantNetwork` and its
  spread; retain the explicit host allowlist, filesystem deny rule, and binding
  allowlist.
- `examples/permissions/README.md:12-18`: describe three composed restrictive
  policies rather than “network granted outright” plus a later override.

The `bind-policy.ts` example may keep an explicit `network: "allow"`; explicit
allow is valid even though it matches the product default.

### Security model

- `website/src/content/docs/docs/security-model.mdx:13-22,140-145` and
  `website/public/docs/docs/security-model.md:9-18,127-132` say everything or
  network is denied until opt-in and describe a secure mixed baseline.

Rename/reframe “Deny by default” as “Sidecar-enforced permissions” or
“Allow-all product default, explicit restrictions.” State that the sidecar
resolves omission to allow-all before installing policy in the kernel, and that
callers running untrusted workloads should pass explicit denies/allowlists. Keep
the accurate isolation facts: no implicit host mounts, guest processes are
virtual, all operations cross the enforcement point, and applied policies cannot
be bypassed.

The page should explicitly distinguish permission posture from capability
existence. An allowed `fs` scope still addresses the VM VFS, and an allowed
`process` scope still addresses kernel-managed guest processes.

### Networking page

- `website/src/content/docs/docs/networking.mdx:52` and
  `website/public/docs/docs/networking.md:29` say the guest cannot reach the
  network by default.

Replace that paragraph with an explicit statement such as:

```text
When `permissions` or its `network` scope is omitted, network operations are
allowed. Pass an explicit `network: "deny"` or rule set to restrict destinations.
Permission is separate from loopback-only listener exposure and the egress/DNS
controls described below.
```

Keep the existing explicit `default: "deny"` allowlist example; it is correct.

### Python runtime page

- `website/src/content/docs/docs/python-runtime.mdx:43` and
  `website/public/docs/docs/python-runtime.md:41` call package downloads
  “default-deny + allowlist.”

State instead that `pip` follows the VM network policy, network is allowed when
the policy/scope is omitted, and callers can pass an explicit deny or allowlist.

### Architecture filesystem and process pages

- `website/src/content/docs/docs/architecture/filesystem.mdx:57` and public
  counterpart line 55 say nothing is bound and filesystem access is denied by
  default.
- `website/src/content/docs/docs/architecture/processes.mdx:36` and public
  counterpart line 34 say process execution is denied by default.

Retain the kernel permission-check step but make the product resolution clear:
the AgentOS sidecar installs `allow` for an omitted scope, while an explicit
policy can deny and returns `EACCES`. Do not weaken the statement that the kernel
always checks the applied policy.

### Main architecture page

- `website/src/content/docs/docs/architecture.mdx`, under
  `### Permissions & approvals` (currently line 303), and
  `website/public/docs/docs/architecture.md`, under the same heading (currently
  line 170), say that the kernel policy means "nothing is allowed until you opt
  in."

Replace that parenthetical with the actual two-layer contract. Recommended
copy:

```text
The lower-level permission policy is enforced by the kernel on every guest
syscall. The sidecar resolves omitted top-level scopes to `allow`; pass explicit
denies or rule sets when the workload needs restriction. Approvals are a
separate layer for an agent asking before it uses a tool.
```

This page was absent from the first Item 38 inventory. It is an Item 38 change,
not Item 51's general architecture cleanup, because it makes the exact same
incorrect permission-default claim as the dedicated permissions page.

### Comparison page

- `website/src/content/docs/docs/versus-sandbox.mdx:19` and
  `website/public/docs/docs/versus-sandbox.md:17` advertise “Granular,
  deny-by-default.”

Use “Granular and sidecar-enforced; allow-all when omitted” or equivalent.

### Already correct; preserve

- `website/src/content/docs/docs/filesystem.mdx:89` and its public counterpart
  correctly say filesystem is granted by default.
- `website/src/content/docs/docs/architecture/networking.mdx:85-111` correctly
  separates permission policy, loopback confinement, and destination/egress
  controls. Its “loopback-only by default” wording is not a permission-default
  claim and must not be banned.
- Explicit rule examples and prose saying `default: "deny"` creates an allowlist
  are correct and must continue to pass the verifier.

## Implementation-ready edit map

Use these replacements in both the source MDX and checked public Markdown copy
where a pair exists. Small prose adjustments for rendering are fine, but retain
the words `omitted`, `allow-all`, and `sidecar` on the required pages so the
positive-claim audit is unambiguous.

| Path / anchor | Concrete replacement behavior or copy |
|---|---|
| `README.md`, `## Why agentOS` security bullet | `**Granular security**: Sidecar-enforced permissions for filesystem, network, process, environment, and bindings. Omitted permissions allow all VM capabilities; pass explicit denies or allowlists for untrusted workloads.` |
| `README.md`, `### Security` permissions bullet | Rename the link to `Sidecar-enforced permissions` and say `Omitted scopes allow access; explicit policies restrict individual scopes and resources.` |
| `permissions.{mdx,md}`, `## Defaults and merge semantics` | Say: `Omitting permissions selects the sidecar-owned allow-all product default. All six scopes—fs, network, childProcess, process, env, and binding—are allow. In an explicit partial top-level policy, omitted scopes also inherit allow. Inside an explicit rule-set scope, an omitted default means deny.` |
| `permissions.{mdx,md}`, scope table | Set every scope default to `allow`; remove the binding auto-grant footnote. Follow the table with: `Allow-all authorizes only capabilities present inside or explicitly configured for the VM; it does not expose the host filesystem, host process table, or unrestricted host sockets.` |
| `permissions.{mdx,md}`, `## Grant or deny a whole scope` | Replace “secure default” with: `Omitted top-level scopes inherit allow, so list every scope that must be restricted.` Keep the explicit `network: "allow"` example only if relabeled as an explicit override; preferably make the example demonstrate `network: "deny"` plus `fs: "deny"` because granting omitted network is redundant. |
| `security-model.{mdx,md}`, first policy heading | Rename to `## Sidecar-enforced permissions`. State that omission is allow-all inside the VM, while host mounts/capabilities are still absent unless configured. Preserve the bullets about virtual processes, mount confinement, and host capability configuration. |
| `security-model.{mdx,md}`, `## Permissions` | Replace the redundant grant example with `permissions: { network: "deny" }` and describe it as restricting the allow-all omission default. Mention that listener loopback confinement is independent from permission mode. |
| `networking.{mdx,md}`, `## Permissions` | `When permissions or its network scope is omitted, network operations are allowed. Pass network: "deny" or an explicit rule set to restrict destinations. Permission is separate from loopback-only listener exposure and the egress/DNS controls below.` Keep the existing explicit `default: "deny"` destination allowlist. |
| `python-runtime.{mdx,md}`, paragraph after the `pip` example | ``pip follows the VM network policy. Network is allowed when permissions or its network scope is omitted; pass an explicit deny or destination allowlist to restrict package downloads.`` |
| `architecture/filesystem.{mdx,md}`, `## Routing a guest syscall`, step 1 | `The kernel checks the installed filesystem policy. The sidecar installs allow when the top-level fs scope is omitted; an explicit deny rejects the operation with EACCES.` |
| `architecture/processes.{mdx,md}`, `## How a spawn is serviced`, step 2 | `The kernel applies the installed VM policy before doing anything. The sidecar installs allow when childProcess is omitted; an explicit deny rejects the spawn with EACCES.` |
| `architecture.{mdx,md}`, `### Permissions & approvals` | Use the replacement paragraph in the section above: omission is sidecar-normalized to allow, explicit policies restrict, approvals remain separate. |
| `versus-sandbox.{mdx,md}`, permissions comparison row | `Granular and sidecar-enforced; allow-all when omitted.` |
| `examples/permissions/server.ts` | Delete the `grant-network` docs region and `grantNetwork` constant, remove `...grantNetwork`, and leave the three explicit restrictive policies (`denyVault`, `allowOneHost`, `allowOneBinding`). No startup or runtime behavior replaces it. |
| `examples/permissions/README.md`, intro/list | Change “four policies” to “three policies”; remove “Network granted outright”; describe the network and binding entries as explicit rule sets whose local `default` is deny. That local phrasing is correct and must not be banned. |

Do not edit `examples/permissions/bind-policy.ts`: its explicit
`network: "allow"` is valid caller input even though it equals the omission
default. Do not edit resource-limit pages merely because they use “secure
default”; those claims concern bounded resource limits, not permissions.

## Claim verifier design

Create `scripts/verify-thin-client-docs.mjs` as an extensible documentation audit
rather than a one-off `rg` wrapper. Item 51 is already expected to add stale
architecture/package/command rules to this same verifier.

### Inputs

Recursively scan only committed public guidance:

- `README.md`
- `website/src/content/docs/docs/**/*.{md,mdx}`
- `website/public/docs/docs/**/*.{md,mdx}`

Use repository-relative, slash-normalized paths and stable sorted diagnostics.
Support `--root=<path>` / `--root <path>` so the test can use fixtures. Missing
required guidance files should be failures, not silent skips.

Match the repository's existing verifier style in
`scripts/verify-fixed-versions.mjs`: synchronous `node:fs`, a local
`defaultRoot`, strict `parseArgs`, an exported audit function, an exported
`main`, and direct execution guarded with `pathToFileURL(process.argv[1]).href`.
The concrete public seam should be:

```js
export function auditThinClientDocs(options = {}) {
  // -> { root, ok, filesChecked, failures: [{ path, line, ruleId, text }] }
}

export function main(argv = process.argv.slice(2)) {
  const result = auditThinClientDocs(parseArgs(argv));
  // stable diagnostics and numeric return code
}
```

Do not shell out to `rg`; direct file reads make the fixture tests portable and
let Item 51 extend the same audit function.

### Forbidden permission-default claims

Strip fenced code blocks before inspecting prose, preserve line numbers, and
report `path:line`, a stable rule ID, and the matching line. Initial rules should
reject at least:

- `deny-by-default` / `deny by default` used as an unconditional product claim;
- `default-deny` used as the product posture;
- “Everything is denied until explicitly opted in”;
- “Nothing is bound by default” in the permission-check context;
- “process execution is denied by default”;
- prose that says the guest/network cannot reach the network “by default”;
- a “secure default” that denies network.

An implementation-ready initial table is:

```js
const forbiddenClaims = [
  { id: "permission-product-deny-default", pattern: /\b(?:deny[- ]by[- ]default|default[- ]deny)\b/i },
  { id: "permission-everything-denied", pattern: /\beverything is denied until (?:explicitly )?opted in\b/i },
  { id: "permission-nothing-allowed", pattern: /\bnothing is allowed until you opt in\b/i },
  { id: "permission-nothing-bound", pattern: /\bnothing is bound by default\b.*\baccess is denied\b/i },
  { id: "permission-process-default-deny", pattern: /\bprocess execution is denied by default\b/i },
  { id: "permission-network-default-deny", pattern: /\b(?:by default[^.]*guest cannot reach the network|network (?:access )?is denied[^.]*opt in)\b/i },
  { id: "permission-secure-default-network-deny", pattern: /\bsecure default\b[^.]*\bden(?:y|ies|ied)\b[^.]*\bnetwork\b/i },
  {
    id: "permission-scope-table-default-deny",
    paths: new Set([
      "website/src/content/docs/docs/permissions.mdx",
      "website/public/docs/docs/permissions.md",
    ]),
    pattern: /^\|\s*`?(?:fs|network|childProcess|process|env|binding)`?\s*\|.*\|\s*`?deny`?\*?\s*\|\s*$/i,
  },
];
```

Apply those rules only to README and website guidance, not `examples/`, so the
correct explanatory comment “Deny all bindings by default” inside an explicit
rule set is not treated as a product claim. Strip a fenced block by replacing
each line from an opening three-backtick or three-tilde fence through its closing
fence with an empty line; do not delete lines, because diagnostics must retain
original line numbers. Sort failures by path, line, then rule ID.

The verifier must not reject:

- fenced examples containing `default: "deny"`;
- prose explaining how an **explicit** rule-set `default: "deny"` creates an
  allowlist;
- generic statements about read-only mounts or loopback-only listeners;
- internal kernel tests, which are outside the public-guidance roots.

A practical implementation is a table of `{ id, pattern, paths? }` rules applied
line-by-line after fence stripping. When `paths` is present, apply the rule only
to that exact set. Avoid one broad `/deny.*default/` expression; it would
incorrectly ban legitimate explicit policy documentation.

### Required positive claims

A pure forbidden-phrase check could pass after deleting all default guidance.
Add required path-specific patterns/fragments for both source and public copies:

- permissions page: omitted top-level policy is allow-all; omitted top-level
  domains inherit allow; rule-set-local omitted `default` is deny;
- security model: AgentOS omission is sidecar-owned allow-all;
- networking page: omitted network permission is allow;
- Python runtime page: omitted network permission is allow;
- main architecture page: omitted top-level permission scopes inherit allow;
- README: omitted permissions are allow-all.

Represent these as a fixed table keyed by exact repository-relative paths,
including all source/public pairs. A missing table key is a
`required-guidance-file` failure; present content missing its phrase is a
`required-allow-all-claim` failure. Required matching should normalize
lowercase plus whitespace but not strip semantic words. This ensures the gate
cannot pass by deleting the permissions/default section.

The initial table must name these 17 paths exactly:

```text
README.md
website/src/content/docs/docs/permissions.mdx
website/public/docs/docs/permissions.md
website/src/content/docs/docs/security-model.mdx
website/public/docs/docs/security-model.md
website/src/content/docs/docs/networking.mdx
website/public/docs/docs/networking.md
website/src/content/docs/docs/python-runtime.mdx
website/public/docs/docs/python-runtime.md
website/src/content/docs/docs/architecture.mdx
website/public/docs/docs/architecture.md
website/src/content/docs/docs/architecture/filesystem.mdx
website/public/docs/docs/architecture/filesystem.md
website/src/content/docs/docs/architecture/processes.mdx
website/public/docs/docs/architecture/processes.md
website/src/content/docs/docs/versus-sandbox.mdx
website/public/docs/docs/versus-sandbox.md
```

For the three architecture/comparison pairs, require the same facts prescribed
in the edit map: an omitted relevant scope installs `allow` on the filesystem
and process pages, and the comparison row says omission is allow-all. This makes
every corrected default claim deletion-resistant, not just the top-level pages.

Use normalized case/whitespace matching so formatting changes do not break the
gate. Make `main()` print
`verify-thin-client-docs: OK (...)` on success or one diagnostic per failure and
exit 1.

### Verifier tests

Create `scripts/verify-thin-client-docs.test.mjs` with Node's built-in test
runner. Cover:

1. the current repository passes;
2. a fixture with “Deny-by-default permissions” fails with rule ID and
   file/line;
3. a reworded “By default the guest cannot reach the network” also fails;
4. deleting a required allow-all/omission claim fails;
5. an explicit `default: "deny"` allowlist explanation and fenced configuration
   pass;
6. the paired public Markdown copy is audited, not only source MDX;
7. unknown CLI arguments fail clearly.

Use a `writeValidFixture(root)` helper that creates the 17 required paths with
minimal valid allow-all claims, then mutate one file per negative test. Use the
exported `auditThinClientDocs` for precise failure-array assertions and
`execFileSync(process.execPath, [scriptPath, "--root", root])` for CLI status,
stderr, and unknown-argument behavior. This avoids copying the whole repository
into each fixture and makes it obvious which claim caused a failure.

The **before** evidence should be produced by adding the verifier/tests first and
running the gate against the uncorrected parent docs. It must exit 1 and enumerate
the known claims above. Then correct the docs and record the same command passing.

### CI integration

Add adjacent steps after the fixed-version verifier in
`.github/workflows/ci.yml:43-44`:

```yaml
- run: node --test scripts/verify-thin-client-docs.test.mjs
- run: node scripts/verify-thin-client-docs.mjs
```

Mirror those commands in `scripts/ci.sh` immediately after the current protocol
compatibility gate (`scripts/ci.sh:34-35`) and before the optional registry
check:

```sh
run_step node --test scripts/verify-thin-client-docs.test.mjs
run_step node scripts/verify-thin-client-docs.mjs
```

Do not opportunistically add other missing CI steps and do not add a root
`package.json` script; project rules reserve root scripts for Turbo
orchestration.

## Validation commands

```sh
# Before correction: expected exit 1 with every stale claim identified.
node scripts/verify-thin-client-docs.mjs

# After correction.
node --test scripts/verify-thin-client-docs.test.mjs
node scripts/verify-thin-client-docs.mjs
node --check scripts/verify-thin-client-docs.mjs
pnpm --dir examples/permissions check-types

# Existing implementation evidence; no runtime source/test edits are expected.
cargo test -p agentos-native-sidecar --lib \
  vm::tests::omitted_permission_domains_use_the_sidecar_allow_all_default
cargo test -p agentos-native-sidecar --test service \
  service::tests::aad_javascript_network_dns_javascript_net_poll_suite -- --exact
cargo test -p agentos-native-sidecar-browser --test wire_dispatch \
  browser_wire_create_vm_without_permissions_defaults_to_allow_all
pnpm --dir packages/runtime-browser exec vitest run \
  tests/runtime/converged-permissions.test.ts --reporter=verbose

# Required documentation render and repository gates.
pnpm --dir website build
pnpm check-types
pnpm lint
cargo fmt --all -- --check
git diff --check
```

The website is currently excluded from the local workspace because its vendored
docs theme is absent. The implementation agent must validate with the same pinned
theme setup used by `.github/workflows/ci.yml:24-29` or record that existing
environment blocker accurately; it must not modify workspace membership as an
Item 38 workaround. Watch `website/scripts/gen-registry.mjs` output and do not
include unrelated generated drift.

## Before/after evidence checklist

| Stage | Exact evidence | Expected result |
|---|---|---|
| Before: implementation behavior | Run the native partial-policy unit, native omitted-policy service test, browser omitted-policy wire test, and browser converged-permissions test named above against the Item 37 parent. | All pass, proving the sidecar already resolves omission/partial top-level policy to allow-all before any documentation edit. |
| Before: documentation defect | Add the verifier first, then run `node scripts/verify-thin-client-docs.mjs` before editing prose. | Exit 1. Diagnostics include both README claims and each stale source/public pair: permissions, security model, networking, Python, main architecture, architecture filesystem/process, and comparison. |
| After: verifier behavior | `node --test scripts/verify-thin-client-docs.test.mjs`, `node scripts/verify-thin-client-docs.mjs`, and `node --check scripts/verify-thin-client-docs.mjs`. | Unit suite passes; repository audit reports `OK`; syntax check passes. Negative fixtures prove exact path/line/rule diagnostics and positive fixtures preserve explicit deny rule sets. |
| After: example/API behavior | `pnpm --dir examples/permissions check-types`. | Removing the redundant `grantNetwork` constant/spread preserves a valid three-policy example. |
| After: rendered docs | Install the pinned docs theme exactly as CI does, then run `pnpm --dir website build`. | MDX, checked snippets, and public rendering build without missing regions or links. In particular, removing the `grant-network` region must be paired with removing its only `CodeSnippet` reference. |
| After: repository gates | `pnpm check-types`, `pnpm lint`, `cargo fmt --all -- --check`, and `git diff --check`. | All relevant gates pass; no generated registry drift, runtime code, lockfile, or workspace-membership edit is included. |

## Risks and boundaries

- **Do not change runtime defaults to make the old docs true.** Item 3 already
  established the sidecar-owned allow-all product default, and native/browser
  parity tests enforce it.
- **Do not delete generic-kernel fail-closed tests.** They cover a different layer
  before AgentOS normalization.
- **Do not conflate network permission with network confinement.** Permission
  omission allows network operations, while VM listener loopback rules, host
  loopback exemptions, DNS pinning, and destination controls remain separate.
- **Do not imply host access.** Allow-all permits operations against VM-owned or
  explicitly configured capabilities; it does not expose unmounted host paths or
  real host processes.
- **Keep source and public copies aligned.** The website build consumes MDX, but
  `website/public/docs` is also a committed public documentation surface and is
  not automatically regenerated by the current website build.
- **Keep explicit restrictive examples.** An allowlist with
  `default: "deny"` is correct; the bug is the claim that omission creates it.
- **Avoid broad unrelated doc cleanup.** Item 51 separately tracks obsolete
  package, command, architecture, and other guidance.

## Dedicated JJ revision scope

Create Item 38 only after Item 37 is sealed, as one direct child revision with a
description such as `docs: correct permission defaults`. Its owned paths should
be limited to:

- `README.md`
- `examples/permissions/server.ts`
- `examples/permissions/README.md`
- `website/src/content/docs/docs/permissions.mdx`
- `website/src/content/docs/docs/security-model.mdx`
- `website/src/content/docs/docs/networking.mdx`
- `website/src/content/docs/docs/python-runtime.mdx`
- `website/src/content/docs/docs/architecture.mdx`
- `website/src/content/docs/docs/architecture/filesystem.mdx`
- `website/src/content/docs/docs/architecture/processes.mdx`
- `website/src/content/docs/docs/versus-sandbox.mdx`
- the eight corresponding paths under `website/public/docs/docs/`
- `scripts/verify-thin-client-docs.mjs`
- `scripts/verify-thin-client-docs.test.mjs`
- `scripts/ci.sh`
- `.github/workflows/ci.yml`
- the Item 38 row/checklist in `docs/thin-client-migration.md`
- this research note if research notes are sealed with their implementations

No Rust/TypeScript runtime source, runtime test, protocol, lockfile, root package
script, workspace-membership, or unrelated website path belongs in the Item 38
revision.
