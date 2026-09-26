# 04: Implement Filesystem and Mounting

**Status:** Implemented; cross-client conformance deferred to step 14

## Outcome

Expose the Core filesystem through thin Rust actions and support durable,
serializable hosted filesystem configuration. Preserve the configurable VFS
without exposing host mounts, arbitrary machine paths, callbacks, or TypeScript
VFS objects.

## Actions

- `filesystem.readFile`
- `filesystem.writeFile`
- `filesystem.readFiles`
- `filesystem.writeFiles`
- `filesystem.stat`
- `filesystem.mkdir`
- `filesystem.readdir`
- `filesystem.readdirEntries`
- `filesystem.readdirRecursive`
- `filesystem.exists`
- `filesystem.move`
- `filesystem.remove`
- `filesystem.export`
- `filesystem.listMounts`

Each ordinary operation reuses the shared Core request and result type and maps
to the same named Core API. The actor adds only transport limits and CBOR byte
encoding. The MVP signatures are:

| Action | Input | Result |
| --- | --- | --- |
| `filesystem.readFile` | `{ path, maxBytes? }` | byte string |
| `filesystem.writeFile` | `{ path, content }` | none |
| `filesystem.readFiles` | `{ paths, maxBytes? }` | per-path content or error |
| `filesystem.writeFiles` | `{ entries }` | per-path success or error |
| `filesystem.stat` | `{ path }` | `VirtualStat` |
| `filesystem.mkdir` | `{ path, recursive? }` | none |
| `filesystem.readdir` | `{ path }` | names |
| `filesystem.readdirEntries` | `{ path }` | typed immediate entries |
| `filesystem.readdirRecursive` | `{ path, maxDepth?, exclude? }` | typed recursive entries |
| `filesystem.exists` | `{ path }` | boolean |
| `filesystem.move` | `{ from, to }` | none |
| `filesystem.remove` | `{ path, recursive? }` | none |
| `filesystem.export` | `{ maxBytes? }` | root snapshot |
| `filesystem.listMounts` | none | live mount descriptions |

`content` accepts either a UTF-8 string or a CBOR byte string. Binary results
are CBOR byte strings rather than integer arrays.

## Hosted filesystem registry

The static binary owns a fixed registry of filesystem implementations allowed in
the hosted API. Configuration selects a registered id and its serializable
config. It cannot register a new implementation at actor creation or runtime.

```text
register(id, config_schema, capabilities, factory)
validate(descriptor) -> ValidatedFilesystemDescriptor
open(validated_descriptor, actor_storage) -> CoreFilesystem
```

The MVP registry is deliberately small and closed:

- Root `default` selects Core's default ephemeral overlay and bundled base.
- Root `actor-sqlite` selects an actor-owned, SQLite-backed root namespace.
- Mount backend `actor-sqlite` selects an actor-owned, SQLite-backed namespace.

The actor DTO never contains the internal `chunked_sqlite` plugin id. It maps
the public descriptor to that plugin only after validation. Immutable artifact
mounts and bounded in-memory mounts require their own later registry entries;
they are not accepted as generic plugin descriptors in this revision.

The generated TypeScript contract uses a closed tagged union matching this
registry. Unknown ids fail even if their config resembles a known backend. The
registry must not contain:

- Host filesystem paths.
- Bind mounts from the actor process or sidecar host.
- File descriptors or sockets.
- JavaScript objects, functions, promises, or callbacks.
- Credentials embedded in a public mount descriptor when a Rivet resource
  binding or operator secret reference can be used instead.
- A generic URL whose scheme can reach the process filesystem or metadata
  services.

Every mount has a normalized absolute guest path, a backend descriptor, and a
read/write mode. The hosted normalizer rejects root, traversal, duplicate,
overlapping, and agentOS-reserved targets and duplicate SQLite namespaces.
Core still performs its normal absolute-path and VM validation when the closed
descriptor is converted to `AgentOsConfig`.

## Durable behavior

Initial `config.filesystem` is stored with actor creation input. Step 11 adds
whole-document `config.set`; there are no separate public mount or unmount
mutations that secretly patch the mount array.

Each config replacement that changes the filesystem follows this order:

1. Validate the descriptor and expected config revision.
2. Prepare or validate the backend without exposing it to the guest.
3. Commit the new desired config and revision.
4. Ask Core to reconcile the live mount table.
5. Record the applied revision or a typed reconciliation failure.

If Core cannot apply a root or mount change to a live VM safely, report
`restart_required`. A failure after the durable commit remains visible in
`config.get`; it must not be silently rolled back to an unknown live state.

`filesystem.listMounts` reads the live Core mount table. The desired descriptor
list is available through `config.get`. Keeping these distinct makes drift
observable.

## Filesystem storage

- Reuse the filesystem owner's existing `agentos_fs_*` namespace and migration
  ladder.
- Do not create filesystem data tables in `agentos_actor_*`.
- Replace any legacy shared component-version mechanism instead of adopting it.
- All filesystem tables are `STRICT`.
- The temporary local SQLite adapter must be swappable for Rivet SQLite without
  changing VFS callers.

## Streaming and limits

- Bound the number of paths and aggregate bytes in bulk reads and writes.
- Bound recursive directory result count, depth, and encoded bytes.
- Bound single-file read and write action payloads.
- Require an explicit maximum for ephemeral storage.
- Keep export within the bounded action limit for the initial implementation.
  The shared large-file streaming mechanism is an explicit follow-up in step 15;
  do not raise transport limits to make large exports fit.
- Preserve POSIX errno for guest path failures. Actor transport validation uses
  distinct typed error codes.

## Tests

- One conformance matrix runs equivalent filesystem operations through embedded
  TypeScript Core, the Rust Core client, and the actor.
- Creation with every allowed root descriptor.
- Replace the mount list, sleep/wake, and verify durable replay.
- Reject host paths, traversal, reserved paths, overlapping mounts, unknown
  descriptor versions, and unsafe URL schemes.
- Concurrent mount updates with `expectedRevision` conflicts.
- Partial backend failure and reconciliation status.
- Bulk, recursive, and export limits.
- Independent filesystem and actor schema migrations in one physical database.

## Acceptance criteria

- All filesystem actions are thin Core calls.
- Safe root and mount configuration can be supplied at creation.
- Every hosted filesystem descriptor resolves through the binary's fixed
  whitelist registry.
- Configured mounts survive actor restart.
- No hosted DTO can identify an arbitrary host path or carry executable host
  behavior.
- Filesystem defaults and path policy exist in one shared Core/VFS layer.
- Desired and live mount states can be compared when reconciliation fails.
- Every filesystem collection and transfer is bounded with typed errors.

## Dependencies and follow-up

Depends on the actor foundation in step 03. Step 11 applies filesystem changes
through full config replacement without adding another mount state store.

## Implementation notes

- Actor action messages are capped at 1 MiB. File and aggregate batch transfers
  are capped at 768 KiB, directory results at 4,096 entries and 512 KiB of path
  data, batches at 128 paths, recursion at depth 64, and paths at 4 KiB.
- The Rust actor uses an agentOS-owned `ActionSet` registry so all actions remain
  individually typed while building against the currently published RivetKit.
  This does not modify RivetKit and is also the registry consumed by the bindgen
  prototype in step 12.
- Runtime access fails with `runtime_not_ready` rather than dereferencing a
  missing or replacement Core VM.
- Creation-time descriptors are normalized into the durable full config. Step
  11 reuses the same normalizer for whole-document replacement.
- Unit tests cover byte encoding, transfer and collection bounds, descriptor
  defaults, reserved targets, overlap, and namespace uniqueness. Live actor and
  TypeScript/Rust Core parity tests remain part of the integration cutover.
