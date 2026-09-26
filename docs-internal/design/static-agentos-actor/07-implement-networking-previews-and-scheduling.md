# 07: Implement Networking, Previews, and Scheduling

**Status:** Implemented; deployment and cross-client conformance deferred to step 14

## Outcome

Expose VM HTTP access, bounded streaming fetches, preview URLs, and scheduled
command execution. Core continues to own guest networking and command semantics;
the actor uses RivetKit's ordinary request and cron APIs for actor-specific
routing, wakeups, and persistence.

## Networking actions

- `network.fetch`
- `network.fetchStream.start`
- `network.fetchStream.read`
- `network.fetchStream.cancel`

`network.httpRequest` remains the shared low-level Core request API but is not a
hosted action. `network.fetch` and its streaming form are actor-safe DTOs for
routing an HTTP request to a VM port. Preserve repeated response headers,
especially `set-cookie`, rather than collapsing them into a map.

## Preview actions

- `network.preview.create`
- `network.preview.expire`

Preview creation stores a random token, VM port, and expiration timestamp in the
actor's SQLite database and returns a path under the actor's ordinary request
gateway. The actor's `on_fetch` handler validates that row and proxies the request
through Core's VM networking API.

There is no separate preview-leasing service. "Lease" means only the current
TTL-bound token row. It survives actor sleep and process restart until expiration.
Expiration is idempotent, and a bounded lazy or scheduled cleanup removes expired
rows. Preview configuration limits token count and maximum TTL.

## Cron actions

- `cron.schedule`
- `cron.list`
- `cron.cancel`

The only target is a serializable process execution descriptor. There is no
session prompt or agent target.

The hosted actor uses RivetKit's existing `ctx.cron()` API for expression
validation, durable storage, wakeups, listing, history limits, and cancellation:

1. `cron.schedule` validates a bounded process execution descriptor.
2. The actor registers a private scheduled action through `ctx.cron().set()` or
   `ctx.cron().every()`.
3. RivetKit wakes the actor and dispatches that action.
4. The scheduled action invokes the same Core process path as `process.exec`.
5. `cron.fired` reports the process id or typed launch failure.

The private dispatch action is omitted from the generated public TypeScript
client. No agentOS cron table, parser, wakeup loop, or Actor Runtime Socket driver
is added. Embedded TypeScript Core may keep its in-process command scheduler, but
the hosted actor does not use it.

## Stream registry

Streaming fetch handles are actor-facing capabilities around Core/sidecar-owned
streams. The sidecar remains the single registry and bounds count, response
bytes, buffered bytes, and idle duration. Actor handles add the runtime
generation and a one-hour absolute expiration and cannot be reused after
restart. This avoids a second actor stream registry.

Every handle is released on:

- End of stream.
- Explicit cancellation.
- Read failure.
- Client disconnect when observable.
- Idle or absolute timeout.
- VM shutdown or replacement.
- Actor shutdown.

The current native stream path requires a kernel-backed HTTP listener. Core
returns a typed error for the legacy in-process listener path rather than
silently buffering the whole response.

Reads return at most the requested bytes capped by an operator maximum. The
actor never reads an entire response into memory to implement streaming.

## Limits and SSRF boundary

- Validate VM port range, method size, header count/bytes, URL size, body bytes,
  concurrent streams, response chunk bytes, preview count, preview lifetime,
  cron job count, and serialized command size.
- `network.fetch` addresses a port inside the actor's VM. It must not become an
  arbitrary host fetch or metadata-service proxy.
- Guest outbound access remains controlled by Core permissions.
- Preview ingress cannot access sidecar control endpoints or actor-internal
  ports.
- Errors preserve guest connection failures separately from Rivet route and
  actor transport failures.

## Tests

- Buffered request status, body, duplicate headers, and errors.
- Streaming response backpressure, partial reads, cancellation, timeout, and
  cleanup after restart.
- Stream count and byte limits.
- Preview create, route, expire, expiry timeout, unauthorized access, sleep/wake,
  and bounded expired-row cleanup.
- Rejection of control ports, invalid ports, unsafe URLs, and oversized requests.
- Cron create/list/cancel, actor sleep/wake, missed wakeup, duplicate wakeup, and
  process launch.
- Cron persistence across process restart using RivetKit storage and no Actor
  Runtime Socket.
- No agent/session scheduled target remains representable.

## Acceptance criteria

- Networking actions delegate I/O to Core's shared reactor capabilities.
- The actor's stream registry is bounded and has no leaked handles.
- Preview routing uses one TTL-bound actor SQLite row and Core networking.
- Command schedules survive actor sleep and do not depend on Actor Runtime
  Socket.
- The actor contains no cron parser, schedule database, wakeup loop, or guest
  networking policy.
- Every networking, preview, and scheduling failure is typed and observable.

## Implementation notes

- Rust Core now exposes buffered and start/read/cancel VM fetch operations.
  Binary request bodies use base64 on the sidecar wire without lossy UTF-8
  conversion, and response headers remain an ordered pair list so repeated
  headers such as `set-cookie` survive.
- The six public network actions use bounded actor DTOs. Stream handles are
  generation-scoped, reads are capped at 128 KiB, and Core/sidecar owns socket
  cleanup and backpressure.
- Preview leases use the actor-owned `agentos_actor_previews` STRICT table and
  schema migration 2. Creation performs bounded expired-row cleanup and an
  atomic 128-token capacity check. The ordinary route is
  `/preview/<token>/...`, strips hop-by-hop headers, and proxies only to the
  lease's VM port.
- Preview TTL defaults to 15 minutes and is bounded between one second and 24
  hours. Expiration is idempotent; unknown or expired route tokens return 404.
- `cron.schedule`, `cron.list`, and `cron.cancel` wrap RivetKit cron directly.
  The only stored payload is a bounded process-spawn descriptor plus its config
  revision. RivetKit validates expressions/time zones and owns persistence,
  wakeups, history, and cancellation.
- RivetKit dispatches the private `__agentos.cron.invoke` action. It is excluded
  from the generated public contract, launches through the existing
  `process.spawn` handler, and emits `cron.fired` with a process handle or a
  bounded error. A changed config revision fails explicitly.

## Dependencies and follow-up

Depends on steps 03 and 05. Preview policy joins the general dynamic config API
in step 11. The TypeScript wrapper for fetch-like ergonomics is generated and
implemented in step 12 without owning stream policy.
