# Limits & Observability

How agentOS bounds resources, applies backpressure, warns before a limit is hit, and surfaces it all to the host.

agentOS runs untrusted, AI-generated code inside disposable VMs. Every resource
that code can consume is **bounded by default**, and every bound is designed to
**warn before it is hit**, **fail with a clear error when it is**, and stay
**inspectable** from one place. This page explains how the limits, backpressure,
logging, and observability pieces fit together across the stack.

## Where limits live

Limits are owned by **secure-exec** (the VM runtime) and **forwarded** by agentOS
— the agentOS layer does not reimplement enforcement, it exposes the knobs and
surfaces the signals.

| Layer | Responsibility |
| --- | --- |
| secure-exec kernel | Enforces per-VM resource caps (memory/heap, CPU time, fds, processes, sockets, filesystem bytes, …). |
| secure-exec sidecar | Owns the bounded queues between the guest, the runtime, and the host; applies backpressure; tracks usage. |
| agentOS client | Forwards `limits` config to the VM and surfaces limit signals to the caller. |

## Three guarantees for every limit

Every bound — a resource cap, a bounded queue, a timeout, a payload size —
follows the same contract:

1. **Bounded by default.** Nothing is unbounded out of the box. Memory is capped
   at ~128 MiB per isolate (Cloudflare Workers parity), CPU is bounded, and every
   queue has a fixed capacity. Operators may *raise* a cap, but never get an
   unbounded default.
2. **Warn on approach.** As usage crosses a threshold (default **≥80%** of
   capacity), a structured warning is emitted — once per crossing, re-armed only
   after it drains back below 50% (hysteresis), so a busy limit logs once, not on
   every operation.
3. **Clear, typed error on breach.** Exceeding a limit produces an error that
   names the limit, the observed-vs-cap value, and the config path to raise it —
   never a bare `EAGAIN`, a silent drop, or a crash.

## Backpressure, not catastrophe

The path from guest code to the host is a **chain of bounded queues**: the V8
runtime → a per-session frame channel → the V8→host event channel → the sidecar
stdout frame queue → the host. When a queue fills (a slow host consumer, a chatty
tool turn), the producer **blocks until the consumer drains a slot** — clean
backpressure that flows all the way back to the guest. A full queue never
destroys the session, silently drops data, or crashes the sidecar; a genuinely
dead consumer surfaces as a typed terminal error instead.

Buffer capacities are sized so that *transient* bursts are absorbed without ever
engaging backpressure; backpressure is the safety net for a genuinely stuck
consumer, not a normal-operation event.

## The limit registry

All bounded limits register with a single in-process **limit registry**. Each
registered limit tracks its live depth, high-water mark, and capacity, and emits
the near-capacity warning described above. This gives the runtime one place to
answer two questions:

- *Is a limit about to be hit?* — the registry fires the approach warning.
- *What is the current usage of everything?* — a registry snapshot lists every
  limit's depth / high-water / capacity / fill-percent for debugging.

A CI audit fails the build if any limit-shaped constant is not classified and —
for operator-tunable ones — wired to a config field, so "is everything bounded
and config-wired?" is verified mechanically rather than by review.

## Logging & host visibility

secure-exec logs to **stderr** (never stdout — stdout is the framed wire
protocol). The default level is `WARN`, tunable with the `AGENTOS_LOG`
environment variable (`error` to quiet, `debug` for per-limit usage snapshots).
Near-limit warnings and backpressure events therefore show up in the sidecar's
stderr stream, which agentOS forwards to the host.

The limit registry also exposes a structured **warning sink**: a callback that
fires on the same edge as the log, carrying `{ name, category, observed,
capacity, fillPercent }`. This is the foundation for host-facing limit
observability — a structured "a limit is approaching capacity" signal rather than
a parsed log line.

## See also

- [Resource Limits](/docs/resource-limits) — the full `limits` config surface.
- [Processes](/docs/architecture/processes) and [Sessions & Persistence](/docs/architecture/sessions-persistence) — the layers the queue chain runs through.