# 15: Follow-up Work After the Actor Refactor

**Status:** Backlog

## Purpose

Track systems that should build on the static actor but should not expand its
initial implementation. These deserve independent designs and revisions after
the actor API is working end to end.

## 1. Production package registry

The initial system does not need a registry protocol: a stable package URL plus
its resolved digest is sufficient. Build a registry later only when friendly
names, discovery, authorization, or update policy justify it. It should resolve
a friendly request into the existing installation input:

```text
resolve(name, selector?) -> { url, digest, size }
```

The selector can begin as an opaque build id or channel. The registry does not
need semantic versioning unless a real consumer requires it. The registry
project owns naming, update semantics, publishing authorization, metadata,
search, bundles, revocation, renewable private-download authorization, CLI
workflows, and production S3 indexing. It does not change Core's
URL-to-verified-package boundary or expose a host path to the actor.

Questions for that design include whether packages have semver at all, whether
channels are mutable, how bundles replace npm meta packages, and what must be
signed in addition to object-store integrity.

## 2. Package installation from the actor filesystem

Evaluate installing a `.aospkg` from a path inside the actor's configured VFS.
This is deliberately absent from the initial hosted API. It needs an immutable
snapshot/pin operation so guest writes cannot change bytes after verification,
plus clear permissions, source lifetime, quota, and cache-sharing semantics.

If added, the hosted actor may expose a closed `FilesystemPath` source that Core
resolves through the actor VFS. It must never mean a host path and must remain a
different wire variant from embedded Core's trusted local `Path` source.

## 3. Large-file streaming

Design one bounded streaming mechanism shared by filesystem reads, writes,
exports, imports, process input/output where applicable, and large network
payloads. Do not grow actor action size limits as a substitute.

The design must cover backpressure, cancellation, runtime-generation scoping,
resume or replay behavior, byte quotas, idle and absolute timeouts, cleanup on
disconnect/sleep/restart, and TypeScript ergonomics.

## 4. Graceful runtime restart and process draining

Define the shutdown behavior deliberately after the actor API is working. The
design should cover admission closure, in-flight action handling, process and
terminal signals, drain deadlines, forced termination, stream cleanup, package
pin release, preview behavior, and typed reporting when the replacement runtime
fails to become ready.

Until this lands, the actor exposes only the guarantees of the existing bounded
Core runtime replacement primitive. Do not infer graceful process semantics from
`vm.restart`.

## 5. Possible RivetKit bindgen generalization

After the agentOS-only prototype proves useful, separately decide whether a
generic RivetKit contract generator is worth building. This is optional future
work, not part of the actor refactor and not required to retire the
product-specific prototype.

If pursued, it needs runtime-neutral action/event schema metadata, an exporter,
and TypeScript generation that understands positional CBOR args, bytes, integer
bounds, tagged enums, optional fields, errors, and dotted action nesting. That
work belongs in a dedicated RivetKit design and revision, not this workspace.

## Acceptance criteria for moving an item out of this backlog

- It has a dedicated design with an owning repository and deployment boundary.
- Its API composes with the static actor without adding policy to actor handlers.
- Its local test story does not require production credentials.
- All storage, network, cache, queue, and streaming resources are bounded.
- It does not add compatibility code for the removed npm, Actor Runtime Socket,
  TypeScript actor, agent, session, or ACP systems.
