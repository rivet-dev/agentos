# 10: Implement the Preload Coordinator

**Status:** Implemented for the process-lifetime MVP. Deployment wiring and
production policy tuning remain in step 14.

## Outcome

Add one central actor that maintains a low-frequency, approximate hot set of
exact agentOS software packages. Each agentOS process reads the set once before
its first actor becomes ready and warms its local package cache within a
deadline. Per-process usage messages are coalesced before being sent to the
coordinator.

This component optimizes cold starts only. It is not a package byte store, lock
service, catalog, source of truth, or requirement for successful actor execution.

## Internal actor API

The internal actor is registered as `agentOS-preload-coordinator` with the fixed
global key `global`. It exposes versioned dotted actions:

| Action | Input | Result |
| --- | --- | --- |
| `preload.getPlan` | Protocol version, process id, supported package-format versions, optional max entries/bytes | Plan revision, exact URL/digest/size package identities, expiry |
| `preload.recordUsage` | Protocol version, process id, monotonic window id, bounded window, coalesced counts | Accepted revision and duplicate/stale disposition |
| `preload.replaceBaseline` | Protocol version and operator-authorized URL/digest/size identities | New revision and bounded baseline totals |
| `preload.status` | None | Bounded coordinator health and plan summary |

`replaceBaseline` is an operator control-plane action. It is registered for
internal administration but is not emitted in the public agentOS actor client.
The coordinator can also receive a baseline in its first creation input; normal
get-or-create behavior ignores creation input after that first creation.

The public `agentOS` actor has no preload action. Its process bootstrap code is a
client of this internal actor.

## Process bootstrap

1. Initialize the process-local package cache and preload service from
   operator-owned startup flags.
2. On creation of the first `agentOS` instance, read `preload.getPlan` with a
   short deadline. A process `OnceCell` shares the attempt and its report with
   concurrent and later instances, including when the attempt fails.
3. Validate protocol version, expiry, format support, exact digests, URLs,
   entry count, and predicted bytes.
4. Fetch entries concurrently through the normal URL resolver and digest cache
   from steps 08 and 09.
5. Stop waiting at the configured startup deadline. Cache single-flight work
   already admitted remains bounded by the cache and downloader deadlines.
6. Publish bounded ready, failed, skipped, byte, deadline, and coordinator
   availability counts through `runtime.status`.
7. Start the actor even if the coordinator is unavailable or optional warming
   is incomplete.

Required packages in an actor's durable config are still acquired on that
actor's startup. They may benefit from warmed bytes but never rely on the
coordinator to identify them. Exact preloads seed the cache's short-lived URL
alias so later URL-only creation input can reuse the digest object.

Only resolved `{ url, digest, size? }` identities are eligible for the plan.
Entries without a pinned digest are not safe preload inputs because URL contents
may change.

## Usage coalescing

Maintain one bounded process observation map keyed by verified digest. Actor
startup and successful dynamic installs increment local counters without
sending a message. A low-frequency timer swaps the map and sends one
`preload.recordUsage` call.

- Distinct keys and per-window counters are capped.
- Distinct overflow increments a bounded dropped-observation counter.
- A process-stable bounded jitter prevents simultaneous flushes.
- Each flush gets one retry under an action deadline; dropping a window is
  acceptable.
- Monotonic process window ids make duplicate and reordered batches no-ops.
- No write occurs for each action, actor start, or package access.
- Graceful process-shutdown flushing is deferred with the broader shutdown work
  excluded from this refactor. Process loss may drop the current window without
  affecting correctness.

## Hot-set policy

The coordinator keeps a small recency-weighted exact map:

- The artifact digest is the ranking key; the most recently observed stable URL
  and exact size are retained for acquisition.
- Usage updates a saturating integer score. Scores halve per six-hour interval
  when ranked, avoiding floating-point and timer-dependent ordering.
- Operator baseline entries reserve the first eligible plan slots.
- Candidates are ordered by decayed score, last observation, then digest.
- Candidate and tracked-process overflow evicts deterministically.
- Entry and predicted-byte limits are applied while producing each plan.
- Plans carry a revision and expiry.

Accuracy is intentionally best effort. Lost batches, evicted process cursors,
temporary coordinator unavailability, and process crashes can affect ranking but
not actor behavior.

## Persistence and limits

The coordinator uses RivetKit durable actor state directly. This keeps the small
bounded ranking model atomic with each action and avoids a duplicate SQLite
mirror. It owns no `agentos_actor_*` tables; those remain exclusive to a VM
actor. If future ranking queries need relational storage, that change must add a
coordinator-specific schema owner.

Coordinator creation config bounds plan entries/bytes, candidates, tracked
process cursors, batch observations, concurrent actions, and plan TTL. Hard
protocol ceilings reject unsafe creation config. Process flags independently
bound plan entries/bytes, warm concurrency, observation entries, read/action
deadlines, startup deadline, and flush interval.

The compiled actor entry point accepts:

- `--preload-startup-deadline-ms`
- `--preload-plan-read-timeout-ms`
- `--preload-max-plan-entries`
- `--preload-max-plan-bytes`
- `--preload-warm-concurrency`
- `--preload-max-observation-entries`
- `--preload-flush-interval-ms`
- `--preload-action-timeout-ms`

## Observability

Structured logs report plan reads, optional acquisition failures, deadline
partial completion, warm totals/bytes, dropped windows, and every near-limit
condition with the creation field or startup flag to raise. Status avoids
package-identity metric labels and exposes only bounded aggregate counters.

## Implemented tests

- The coordinator registers exactly four internal dotted actions.
- Baseline slots precede candidates and ranking is deterministic.
- Duplicate and reordered process windows do not change score or revision.
- Candidate and tracked-process collections evict at configured bounds.
- Repeated local observations coalesce by digest; distinct overflow increments
  the drop counter.
- Process configuration rejects zero, incoherent, and above-protocol bounds.
- Exact resolution seeds the advisory source alias, so a later unresolved
  source reuses the warmed artifact.

Malformed-plan, deadline, coordinator-unavailable, periodic-flush, and
multi-process behavior remain deployment integration coverage for step 14. The
MVP code treats each failure as optional and never bypasses required package
resolution.

## Acceptance criteria

- Optional hot-package bytes can be available before the first actor needs
  them, within the configured startup budget.
- Coordinator reads occur once per process startup, not once per actor.
- Writes are coalesced per process and low frequency.
- The coordinator never proxies package bytes.
- No correctness, authorization, or desired state depends on its plan.
- Every map, message, plan, timer, counter, concurrent action, and warm operation
  is bounded.

## Dependencies and follow-up

Depends on the URL package source and cache from steps 08 and 09. Step 14 deploys
and operates the coordinator alongside the static actor and supplies the
deployment-level failure tests.
