# Thin Rust actor refactor

Status: implementation in progress.

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

- The sidecar owns defaults, canonical VM config normalization, validation,
  live-versus-restart classification, replay buffers, package acquisition,
  package cache identity, eviction, pins, and projection.
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
- [ ] Move public DTOs and the action manifest into the contract crate, reusing
  Core/protocol DTOs wherever their semantics match.
- [x] Declaratively register the action surface from the action definitions,
  without a second handwritten registry. Continue by generating the repetitive
  pass-through handler bodies.
- [ ] Generate pass-through forwarding without a
  second handwritten action registry.
- [x] Keep contract version, hash, capabilities, TypeScript output, and snapshots
  deterministic.

### Core and sidecar ownership

- [ ] Put complete config defaulting, normalization, validation, and change
  classification behind one sidecar operation.
- [ ] Make filesystem actions direct bounded Core operations.
- [x] Make process and terminal replay pages direct Core results backed by one
  bounded cursor implementation; keep events as
  hints and replay as authoritative.
- [ ] Make language actions direct Core operations using shared transport-safe
  options and results.
- [ ] Keep pull-based fetch response streaming, with chunk state owned below the
  actor.
- [ ] Finish sidecar-only package fetch, verification, single-flight, cache,
  eviction, pinning, and projection.

### Irreducible actor orchestration

- [x] Separate the preload coordinator and process preload service from
  `AgentOsActor` in the `agentos-preload` crate.
- [ ] Reduce configuration handlers to revision checks, persistence, sidecar
  resolution/classification, and reconciliation.
- [ ] Keep URL-only software and closed filesystem registry enforcement at the
  hosted boundary; reject host paths and callbacks.
- [ ] Keep cron private invocation, preview TTL ownership, and bounded RivetKit
  event bridging in the actor.
- [ ] Remove remaining actor-owned domain validation after the lower-layer
  operation is authoritative. Generic transport, replay paging, and package
  cache behavior are no longer implemented by the primary actor.

### Verification

- [ ] `cargo check --workspace`
- [ ] Focused actor, contract, client, sidecar, VFS, config, replay, package, and
  SQLite tests.
- [ ] Generated contract determinism and snapshot tests.
- [ ] `pnpm build` and `pnpm check-types`.
- [ ] Core and inspector tests.
- [ ] Fixed-version and publish-helper checks.
- [ ] Public documentation build and link checks.
- [ ] Embedded and hosted defaults remain ephemeral unless explicitly configured
  with a durable filesystem.
- [ ] Hosted storage uses RivetKit SQLite directly and performs no agentOS-owned
  S3 calls.

## Merge blockers

- Do not merge while local SQLite or Actor Runtime Socket remains in a hosted
  production path.
- Do not merge while actor-local config classification, replay state, or package
  cache behavior is authoritative.
- Do not merge without proving generated Rust and TypeScript contracts remain in
  lockstep.
