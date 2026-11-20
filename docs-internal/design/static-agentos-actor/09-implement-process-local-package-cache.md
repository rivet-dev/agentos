# 09: Implement the Process-Local Package Cache

**Status:** Implemented for the process-lifetime MVP. Persistent cache recovery,
HTTP validator indexes, and explicit warm orchestration remain deferred.

## Outcome

Add a bounded process-local content-addressed cache for verified `.aospkg`
artifacts. All actor instances in one process share acquisition, validation, and
immutable bytes. The cache is a performance layer, not durable desired state and
not an authorization boundary. Its result is a pinned immutable package handle
that Core can memory-map and project, never an actor-visible operating-system
path.

## Component API

Core exposes process initialization and bounded status, while package resolution
uses an internal async interface equivalent to:

```text
configure_process_package_cache(options) -> result
PackageResolver.resolve(source) -> VerifiedPackage
get(digest) -> VerifiedPackage | CacheMiss
get_or_acquire(flight_key, expected_digest?, acquisition) -> VerifiedPackage
drop(VerifiedPackage) -> release pin
stats() -> CacheStats
```

Callers never read or write a cache path directly. `CachedArtifact` returns a
read-only mmap/blob handle supported by Core/VFS package projection. Extending
the actor API to accept a private host cache path is explicitly out of scope.

`VerifiedPackage` is the pin. Cache entries and live installations share the
same immutable artifact handle; eviction is legal only when the cache owns the
last handle. Step 10 builds bounded `warm` orchestration on top of exact-digest
`PackageResolver.resolve` calls.

## Storage model

- Key immutable objects by verified `.aospkg` digest, not a mutable object-store
  path.
- Acquire into a uniquely created private staging object behind the cache
  implementation.
- Verify before atomically publishing an immutable digest entry.
- Treat existing digest objects as immutable. A size or content mismatch is
  corruption, not a reason to overwrite silently.
- Maintain a separately bounded, short-TTL advisory source-key index so actors
  resolving the same URL immediately after a preload can reuse its digest.
- Keep actor-specific writable filesystem overlays outside this cache.
- Store cache objects in a private process-lifetime temporary directory with
  read-only permissions after publication.

Neither the actor contract nor the Core package source DTO can name, open, or
mount cache storage as a host path. Trusted embedded Core can inspect the pinned
path already required by the sidecar projection boundary.

Persistent local cache recovery and a bounded URL/ETag index are not part of
this MVP. The central coordinator in step 10 supplies a fresh process with exact
URL/digest identities to warm, so persistence is an optional later optimization
rather than a correctness dependency.

## Single-flight behavior

Concurrent requests for the same known digest share one bounded acquisition
future per process. When the digest is omitted, concurrent requests for the same
normalized URL (or trusted path string) share a short-lived resolution flight
and then join the digest entry. Requests for different content use a global
acquisition semaphore.

- Success wakes all waiters with the same immutable handle.
- Failure wakes all waiters with the same typed failure and removes the flight so
  a later bounded retry can occur.
- Waiter cancellation does not cancel a fetch still required by other waiters.
- A flight has a configurable deadline and cannot remain permanently registered.
- URL-resolution flights are advisory and never make URL text the immutable
  content key.
- The total number of distinct pending flight keys is bounded separately from
  active acquisitions, preventing unique URLs from growing an unbounded map.

## Eviction and pinning

Use byte-bounded LRU eviction locally. Track last access and size, but do not
persist high-frequency access synchronously.

- Pinned artifacts used by a live VM cannot be evicted.
- A cache insertion that cannot meet its byte limit after evicting unpinned
  entries fails with a typed capacity error.
- Over-limit pinned bytes are visible in metrics and status. The cache does not
  delete a live package to force itself under limit.
- Different URLs that resolve to the same digest store one object.
- The private temporary directory is released with the process. Persistent
  staging cleanup is deferred with persistent cache recovery.

## Process ownership

Initialize one cache service for the process, outside individual actor state.
Actor shutdown releases only its pins. Process shutdown performs a bounded flush
of access metadata and usage observations but does not make correctness depend
on that flush.

The cache configuration is operator-owned and first-write-once per process:

- Maximum bytes and object count.
- Maximum concurrent resolutions and downloads.
- Maximum distinct pending acquisitions.
- Acquisition timeout.
- Maximum advisory source-index entries and their short TTL.

No actor action may raise these process-wide limits.

The compiled actor entry point accepts:

- `--package-cache-max-bytes`
- `--package-cache-max-entries`
- `--package-cache-max-concurrent-acquisitions`
- `--package-cache-max-pending-acquisitions`
- `--package-cache-acquisition-timeout-ms`
- `--package-cache-max-source-entries`
- `--package-cache-source-ttl-ms`

Embedded Core may call `configure_process_package_cache` before constructing a
resolver. Repeating the same options is idempotent; changing options after the
first resolver exists returns a typed configuration error.

## Observability

`process_package_cache_stats` reports entries, advisory source entries, bytes,
pinned entries, pending acquisitions, hits, misses, coalesced waiters,
acquisitions, evictions, and capacity failures without package labels. Near
pending-key, byte, object, and source-index limits emit structured tracing
warnings naming the startup option to raise.

Every background cleanup or index flush failure is logged. Corruption is
quarantined or removed with an explicit record; it is never treated as a cache
hit.

## Implemented tests

- One hundred concurrent same-source requests cause one acquisition and share
  one digest artifact.
- A recent unresolved source-key lookup reuses the digest entry through the
  bounded short-TTL index.
- An exact digest preload also seeds its bounded source alias, allowing a later
  unresolved lookup of the same URL to reuse the warmed digest object.
- A failed flight is removed before its callers wake, so an immediate retry
  starts a fresh acquisition.
- A timed-out flight releases its pending slot and permits an immediate retry.
- Distinct pending keys hit the typed configured bound.
- A pinned package blocks entry eviction; releasing its handle permits LRU
  eviction and removes the old immutable file.
- Path and URL sources with identical bytes converge on one digest entry.
- Digest mismatch and malformed package validation remain typed.
- Actor entry-point cache flags parse positive bounded values.
- The real sidecar install/uninstall test projects from the cached immutable
  path and releases the live installation pin.

Step 10 adds warm deadlines and actor-startup behavior. Persistent cache
recovery, crash-shaped staging cleanup, URL validator indexes, and explicit
corruption quarantine remain follow-up optimizations if measurements justify
them.

## Acceptance criteria

- Package bytes are acquired and verified at most once concurrently per
  process for a known digest or normalized unresolved source key.
- Cache entries are immutable and content-addressed.
- Live VM packages remain pinned.
- Disk, memory, concurrency, cleanup, and metadata are bounded by default.
- Distinct pending acquisitions are bounded independently from active network
  work and fail with an error naming the startup option to raise.
- An empty or failed cache only affects latency; exact desired artifacts remain
  available from their durable URL and pinned digest.
- Cache internals do not appear in actor creation or mutable config.

## Dependencies and follow-up

Depends on the URL/path-to-verified-package contract in step 08. Step 10 reads a
central advisory hot set and resolves exact digest-pinned sources through this
cache; it does not bypass the cache or introduce a host path.
