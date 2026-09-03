# Thin Rust actor refactor

Status: thin-actor extraction and the required lower-layer ownership moves are implemented.

The static `agentOS` actor is a RivetKit lifecycle adapter over agentOS Core and
the native sidecar. It must not become a second implementation of VM behavior.
The hosted actor continues to use the ephemeral bundled-base overlay by default;
durable filesystem storage remains explicit configuration.

## Baseline

Before this refactor, `crates/actor/src` contained approximately 9,900
production lines and 12,500 lines including inline tests. Line count is only a
diagnostic. A change is complete only when the lower layer becomes the single
behavioral owner.

| Current area | Baseline production lines | Final owner | Actor responsibility |
| --- | ---: | --- | --- |
| Action transport and contract | 604 | `agentos-actor-contract` and bindgen | Register the generated action set |
| Configuration | 994 | Sidecar normalization plus actor durable intent | Revision checks, persistence, and reconciliation |
| Runtime lifecycle | 548 | Core plus actor orchestration | Boot, restart, sleep, and destruction |
| Filesystem actions | 700 | Core/sidecar | Hosted boundary checks and forwarding |
| Processes and terminals | 1,201 | Core/sidecar | Generation routing and event bridging |
| Language execution | 1,701 | Core/sidecar | Generation routing and forwarding |
| Networking and previews | 556 | Core plus Rivet preview ownership | Preview ownership and bounded forwarding |
| Software mutations | 314 | Sidecar package service | Desired-config mutation and reconciliation |
| Preload coordinator/process warmup | 1,613 | Separate preload coordinator and sidecar package service | Advisory warmup coordination only |
| Actor SQLite adapter | 587 | RivetKit SQLite capability | Narrow callback adapter and actor-owned schema |
| Cron | 391 | RivetKit scheduling plus Core execution | Schedule persistence and private invocation |
| Events and metrics | 304 | Contract transport plus actor bridge | Bounded RivetKit event emission |

The extraction leaves approximately 3,980 production lines and 5,631 total lines in
`crates/actor/src`, down from approximately 9,900 and 12,500 respectively. The
public config, lifecycle, action, event, and transport DTOs plus their pure
Core adapters live in `agentos-actor-contract`. The process preload service,
central coordinator actor, and their metrics/tests live in `agentos-preload`;
`crates/actor/src/preload.rs` is a 22-line lifecycle adapter.

The largest remaining actor files are the Rivet SQLite adapter (587 production
lines), lifecycle controller (487), process/terminal routing and event bridging
(521), filesystem forwarding (391), and language forwarding (336). The hosted
SQLite path uses `Ctx::sql()`; `rusqlite` is confined to an opt-in adapter test.

## Ownership rules

- The sidecar owns VM defaults, canonical VM config validation and restart
  classification, language replay, ordinary-process and terminal replay,
  package acquisition/cache, and package projection.
- Rust and TypeScript Core expose behaviorally identical operations over the
  sidecar. Shared request/result DTOs live with Core or the protocol rather
  than being copied into actor handlers.
- The contract layer owns TypeScript-compatible CBOR decoding, output envelope
  limits, public error conversion, the action manifest, schema export, and
  deterministic contract metadata.
- The actor owns Rivet lifecycle, durable desired config revisions, optimistic
  concurrency, Rivet SQLite capability routing, cron invocation, preview
  ownership, hosted-only trust restrictions, and bounded event bridging.
- Services imports and registers the product-owned actor. It does not own an
  agentOS implementation.

## Checklist

### Evidence and contract foundation

- [x] Record the responsibility and line-count baseline.
- [x] Move generic CBOR compatibility, response bounding, and public error
  conversion out of the actor implementation.
- [x] Move public DTOs and the action manifest into the contract crate, reusing
  Core/protocol DTOs wherever their semantics match.
- [x] Declaratively register the action surface from the action definitions,
  without a second handwritten registry.
- [x] Generate repetitive language pass-through forwarding without a second
  handwritten action registry. Filesystem, process, and network handlers remain
  explicit where they enforce action budgets or bridge Rivet context.
- [x] Keep contract version, hash, capabilities, TypeScript output, and snapshots
  deterministic.

### Core and sidecar ownership

- [x] Put config defaulting, normalization, validation, mount canonicalization,
  software restart identity, and change classification behind one sidecar
  operation. The actor persists desired intent and asks the sidecar whether a
  live VM generation remains equivalent.
- [x] Make filesystem actions direct bounded Core operations.
- [x] Make Rust actor process and terminal replay pages direct Core results;
  keep events as hints and replay as authoritative.
- [x] Move ordinary-process and real-terminal replay retention, sequencing,
  cursor/truncation behavior, and exit status into one bounded sidecar store.
  Rust and TypeScript process readers delegate to it, client-local replay
  buffers are removed, and configured VM page limits can be raised without
  changing clients. TypeScript Core's synthetic prompt terminal remains a
  local compatibility surface and does not claim recoverable replay.
  Review regressions cover pending-launch reservations/cancellation, VM-owned
  omitted page bounds, typed replay failures, and inspector cursor advancement
  over dropped trailing output. Public explicit zero remains invalid; v11
  uses zero only as an internal wire omission sentinel.
  Ordinary replay also reconciles confirmed completion into Core wait/status
  and the actor's `end` field after a missed live exit notification.
  Real-terminal replay similarly repairs live/retained shell completion after
  a lost notification or earlier observation failure, without duplicating
  retained entries or keeping the event observer alive.
- [x] Unify language-spawn registry admission with ordinary process admission in
  both Rust and TypeScript Core. Pending asynchronous admissions count against
  the shared 1,024-entry cap, and oldest exited entries are evicted under
  pressure before a typed resource-limit error is returned. Language
  subscriptions start before the admission RPC to retain fast
  output/completion. Both Rust and TypeScript language-process reads use the
  sidecar's execution replay. This is the existing `ReadExecutionOutput` route
  with a client byte-page adapter, separate from ordinary/terminal replay's
  `ReadProcessOutput` route. Live output metadata is preserved by both Core
  clients; TypeScript's shared event subscription filters VM ownership.
- [x] Make language actions direct Core operations using shared transport-safe
  options and results.
- [x] Keep pull-based fetch response streaming, with chunk state owned below the
  actor.
- [x] Route normal Core installs and actor/preload acquisition through the native
  sidecar, which executes fetch, verification, single-flight, cache, eviction,
  pinning, and projection. The resolver/cache implementation is still physically
  in the client crate, reused by the sidecar; this is code-location debt, not a
  host-client cache in the install/preload path. Embedded trusted local-path
  sources remain supported; hosted sources remain URL-only.

### Irreducible actor orchestration

- [x] Separate the preload coordinator and process preload service from
  `AgentOsActor` in the `agentos-preload` crate.
- [x] Reduce configuration handlers to revision checks, persistence, sidecar
  resolution/classification, and reconciliation.
- [x] Keep URL-only software and closed filesystem registry enforcement at the
  hosted boundary; reject host paths and callbacks.
- [x] Keep cron private invocation, preview TTL ownership, and bounded RivetKit
  event bridging in the actor.
- [x] Remove generic transport, language/process/network DTO conversion, replay
  paging, and package-cache behavior from the primary actor. Filesystem action
  size/path bounds remain hosted-contract enforcement rather than VM semantics.

### Verification

- [x] `cargo check --workspace`
- [x] Focused actor, contract, client, sidecar, VFS, config, replay, package, and
  SQLite tests.
- [x] Generated contract determinism and snapshot tests.
- [x] `pnpm build` and `pnpm check-types`.
- [x] Core and inspector tests.
- [x] Fixed-version and publish-helper checks.
- [ ] Public documentation build and link checks.
- [x] Embedded and hosted defaults remain ephemeral unless explicitly configured
  with a durable filesystem.
- [x] Hosted storage uses RivetKit SQLite directly and performs no agentOS-owned
  S3 calls.

The public documentation sources were updated, but their validation gate is
not currently usable in this checkout: `just docs-check-links` matches no
`@rivet-dev/agentos-website` workspace package, renders no files, and then
reports success after checking an empty `website/dist`. This is recorded as an
unresolved repository validation issue rather than counted as a passing gate.

## Merge blockers

- Do not merge if local SQLite or Actor Runtime Socket re-enters a hosted
  production path. The current `rusqlite` dependency is test-only.
- Do not merge without proving generated Rust and TypeScript contracts remain in
  lockstep.
