# Separate Node fork IPC from ordinary descriptors

Status: isolated implementation authorized, with independent code review and
finite tests required before parent integration. Revised after Astra's concrete
review requirements: no recurring idle polls, explicit interest-change wake,
generation-bound operations, and aggregate/transient descriptor admission. This is
the owning fix design for the confirmed STAB223 compatibility failure. No native
build or broad suite is authorized by this document.

## Actual evidence

The immutable v6 sidecar has SHA256
`fba321affe8a04cb4976600812cbc22b4c4d9285902d7cc28c455c1eda85b7ab`, source
`6d47493a7cabb110cd6a69192c32b17d9084e198`, size 160380040, mode 0555 and
link count 1. Its `target/parity-native-v6/stdin-observations.json` preserves
both Node 26.13.0 and guest observations; the owner retired without truncation.

- Ignored stdin: Node exposes null parent stdin and immediate fd0 EOF. The guest
  exposes a stream, broadcasts IPC bytes to public stdin listeners, and reports
  EBUSY for the direct fd0 read.
- Piped stdin: Node observes only USER, then EOF, then a second IPC reply. The
  guest observes control bytes plus USER; stdin.end destroys the IPC writer,
  the second send fails ERR_STREAM_DESTROYED, and the finite guard kills it.

The successful v6 echo case remains valid evidence of startup and echo only.
The source-only SharedArrayBuffer change is unrelated and is not in this binary.

## Facilities and constraints

| Existing owner | Facility relevant to the repair |
| --- | --- |
| vm-kernel/kernel.rs:5575 | fd_socketpair creates connected kernel AF_UNIX stream endpoints. Open descriptions own their socket lifetime; existing fd, socket, connection and buffered-byte limits apply. No host network connection is needed. |
| vm-kernel/kernel.rs:5949 | fd_transfer and fd_install_transfer preserve the description when moving an endpoint into another process. fd_description_identity identifies a descriptor independently of its reusable number. |
| vm-kernel/kernel.rs:7963,8120,8314 | Zero-time descriptor reads, nonblocking writes, poll_fds and poll_wait_handle allow owner-thread probes without blocking the actor. |
| vm/execution/stdio.rs:650,692 | Existing pipe and /dev/null installation already implement the two required fd0 modes. They must continue to own ordinary input. |
| vm/execution/host_dispatch/filesystem.rs:164 | Ordinary reads retain one deferred_kernel_read per process. IPC must not occupy this slot. |
| vm/state.rs:104; host_dispatch/network_compat.rs:420 | Deferred polling already demonstrates generation/PID validation, direct reply claiming, notifier-only wait tasks and zero-time actor rechecks. Ordinary deferred_kernel_poll must also remain available. |
| vm/execution/child_process.rs:10992,12775 | Descendants have explicit owned process paths and separate dispatch/service paths. Root-only changes are insufficient. |

The current net.Socket constructor does not adopt options.fd. Adding that public
API and integrating IPC into managed host-network resources would enlarge this
repair and couple local IPC to network policy. The proposal uses descriptors
directly, with a separate internal process capability.

## Proposed transport and authority

1. Normalize fork's existing Node stdio option to include exactly one ipc entry.
   Preserve the first three entries, including ignore, pipe, inherit and numeric
   descriptors. Reject duplicate IPC entries before allocating resources. Remove
   the v6 override that forces fd0 to pipe. child.stdin remains null for ignore.
2. At the final child-spawn descriptor setup, after ordinary inherited descriptors
   and spawn actions have been applied and before starting user code, allocate
   one nonblocking kernel stream socket pair. Install one endpoint in the parent
   and transfer the other into the child's requested IPC descriptor slot. Close
   temporary aliases immediately. Both endpoint descriptors are CLOEXEC so later
   unrelated spawns/execs cannot retain an accidental writer. The child's initial
   endpoint must be installed after its own descriptor inheritance/exec sweep.
3. Retain an internal endpoint record in each owning ActiveProcess: kernel fd,
   open-description identity, channel generation and peer identity. The parent
   indexes its endpoints by its existing owned child ID; the child has a single
   endpoint to its parent. The parent endpoint is not present in the child's fd
   table. Allocation/transfer/startup rollback closes both sides and releases all
   reservations, including failure after installing only one endpoint.
   Do not use fd_install_transfer_at as an admission check: it validates the
   index but does not enforce aggregate FD growth. Use the existing accounted
   fd_install_transfer into the child. Close the parent's temporary peer alias
   after successful child installation, then fd_dup2 if its IPC slot differs,
   restore CLOEXEC and close the temporary child alias.
   fd_socketpair admits two FDs; fd_install_transfer and a growing fd_dup2 each
   admit their actual transient FD against the current kernel snapshot. The
   sequence can require three simultaneous FDs for two final endpoints; failure
   at that transient limit rolls back rather than bypassing the cap. Track every
   installed alias and transfer guard, drop the guard before the last cleanup
   close, and preserve pre-existing destination ownership on failed admission.
4. Derive IPC availability from these owner records through an internal info
   operation. AGENTOS_NODE_IPC in guest env must not grant a channel. Carry the
   existing serialization choice through the typed launch request/info reply,
   rather than introducing an env setting. This adds internal lockstep contract
   fields, not a new SDK option or new Node API.
5. Introduce typed internal process operations IpcInfo, IpcRead, IpcWrite,
   IpcPoll, IpcPollUpdate and IpcClose. An endpoint selector is Self or an owned child ID, never
   an arbitrary PID, raw fd or host path. Resolve it against the authenticated
   execution's process tree. IpcInfo returns the channel generation; IpcRead,
   IpcWrite, IpcPoll interest entries and IpcClose must carry that exact generation.
   Reject a stale queued request before touching a replacement endpoint, even
   when its owned child selector or descriptor number matches. Also validate
   current process generation and description identity on every access.
   A raw guest close/reuse of its IPC fd must yield disconnected
   channel behavior; cleanup must never close a replacement descriptor.

These operations stay inside the guest kernel. They require no host executable,
host socket, SDK request to relay messages, or networking permission exception.

## Independent readiness and bounded buffering

IpcRead and IpcWrite are synchronous zero-time probes, capped to 64 KiB per call
and existing smaller kernel limits. They return bytes/EOF or a partial byte count
and would-block status. They never retain an unbounded native write backlog.

Use one separate deferred IPC poll slot per owning process. A single guest IPC
pump submits its bounded set of live endpoint selectors and read/write interests;
only endpoints with pending output request writable readiness. This supports
multiple children without one parked read per child or competition with fd0.
The selector list is bounded by the caller's live channel count and configured
process/fd limits. Repeated concurrent private polls receive typed EBUSY.
When send adds output during a parked read-only poll, replace the interest set
through IpcPollUpdate with the current poll ID, expected interest revision and
generation-bound endpoints, and wake the existing waiter. Do not wait for readability or an unrelated kernel
notification to discover writable demand. An interest revision fences an older
completion; update/acknowledgement must not admit two concurrent poll owners.
Use this same path when endpoints are added or removed. Test the read-only idle
wait followed by a send against a peer that sends no incoming bytes.

The poll implementation follows existing direct-reply ownership: validate and
claim once; snapshot kernel readiness before a zero-time poll; retain only a
notifier/deadline task; wake the process owner; revalidate and probe on that owner;
settle once. An idle waiter has deadline None, as existing deferred POSIX polling
supports. It remains cancellable and owns one bounded interest set and one task;
it does not return an empty result every 50 ms or resubmit periodic discovery.
Unrelated notifier wakes recheck durable readiness and rearm the same claimed
reply if no requested endpoint is ready. Explicit operations, disconnect or
cancel may have finite deadlines. Do not call blocking kernel poll on the actor,
spin microtasks, or add interval/timeout loops for idle readiness discovery.
Admission, channel creation/close and teardown must notify the same waiter to
avoid missed changes. Cancellation/teardown aborts and joins the waiter task and
settles its exact direct reply before retiring the process owner.

The implementation parks the reply in process-owned state and returns from host
service preparation immediately, releasing its admission permit. One VM-scoped
notifier task retains only the kernel wait handle, private interest-change
notifier and durable owner-event route. It queues the existing deferred-poll
recheck event with the owning descendant path. Root and descendant owner hooks
probe IPC alongside ordinary deferred operations; IPC never waits on the broker
notification that its own wake task publishes. Teardown aborts the registered
notifier and fails the claimed reply before joining the execution. A long-lived
service future must not occupy an admission slot while IPC is idle.

The bridge keeps an explicit offset for partial writes. Apply the existing
pre-ready queue bounds (1024 messages, 8 MiB) throughout the channel's lifetime,
in both directions, rather than dropping them after ready. Bound the incomplete
frame buffer to 8 MiB including its framing bytes; reject an oversized frame
before copying/enqueueing it. Kernel socket buffered-byte accounting remains
independent and enforced. send returns false at pressure, and callbacks settle
once on complete transport write or a typed terminal error. No silent drop,
retry-as-success or enlarged resource limit is part of this change.

Keep the current message serialization for this repair. Parse its frames only on
the private channel, with persistent decoding across chunks and strict size
bounds. Remove IPC parsing from stdout and the ordinary stdin data listener.
Both ordinary output streams then preserve arbitrary user bytes, including text
that resembles an IPC frame. Advanced serialization restrictions remain separate.

## Lifetime and implementation boundaries

Endpoint states are open, draining and closed. Explicit disconnect stops new
sends, drains accepted output, closes the private descriptor and emits disconnect
once; peer EOF or a terminal error settles pending callbacks and closes once.
stdin.end and close(0) affect only stdin. Closing IPC never closes fd0/1/2.
Natural exit, signal, startup failure, exec replacement and VM disposal abort the
private poll, settle/fail its direct reply, release queued bytes and close only
the recorded endpoint descriptions. Pending reads/writes must not retain V8 or a
process after its owner retires. Preserve existing IPC ref/unref listener behavior.

Close authorizes against the retained selector and channel generation before
examining the current descriptor. An already closed descriptor or a different
open description still permits retirement of that exact metadata record; a
replacement descriptor remains untouched. Read, write and poll continue to
require the recorded description identity. A stale channel generation cannot
retire a newer record. Unexpected kernel close errors retain ownership and
propagate rather than silently abandoning the record.

Retained endpoint records have their own finite admission check because an
ordinary fd close frees descriptor capacity without retiring IPC metadata.
Before publishing either endpoint, commit checks each owner's combined parent
and child record count against the existing descriptor-table limit
(`max_open_fds`, with the kernel's 1024 fallback). A full owner returns EMFILE;
duplicate endpoint ownership returns EBUSY. Rejection leaves the prepared
descriptors in the startup rollback guard and rolls back the unregistered child.
This adds no public setting and does not raise descriptor limits.
The retained-record owner emits a structured warning at 80% of the bound,
independently of the ordinary FD gauge. It warns once until the record count
falls below that threshold, then rearms. Warnings and EMFILE details identify
`limits.resources.maxOpenFds`, observed records and the bound, and advise closing
unused IPC channels or raising that existing limit. Finite successor controls
cover threshold/reset and actual root/nested startup rejection; they remain
unrun until independent review and a qualified native build.

The deferred JavaScript close callback settles its internal closed promise in a
finally block. A throwing user disconnect listener still propagates its original
error, while peer-exit ownership can finish and native close remains once-only.
Pure transport RED/GREEN proves this settlement; native metadata RED proves the
ordinary-close, replacement and unbounded-record failures. Native GREEN remains
required before accepting these source corrections.

| File area | Planned change |
| --- | --- |
| executor-contract/host/process.rs; vm-host-interface; host_dispatch inventory/decoder; javascript/rpc.rs | Typed private IPC requests, bounded decoding, trusted channel info and bridge callbacks. |
| vm/state.rs; execution/process.rs | Endpoint ownership and one private deferred poll, initialization and terminal cleanup. |
| execution/child_process.rs; new execution/process_ipc.rs | Shared root/descendant channel allocation, authorized operations, rollback and nonblocking service. |
| execution/process_events.rs | Narrow recheck/wake hook for the private poll, including descendants. No change to public output queue admission/accounting. |
| bridge builtins/child-process.ts, process.ts; new internal IPC helper | Dedicated transport pump; normal stdio setup; remove stdin/stdout multiplexing and env authority. |

process.rs and process_events.rs also contain parent-owned STAB217 work. All
changes remain in this isolated checkout; the parent should review the narrow
lifecycle hooks during independent code review before integration. No service.rs queue change is
planned. If the trace requires a wider ownership change, revise this design first.

## Validation required before acceptance

Preserve all existing RED artifacts. Add finite kernel/owner controls for
half-installed rollback and aggregate/transient FD admission, independent fd0 and
IPC close, descriptor-number reuse, stale queued channel generations, partial
writes, would-block wake, read-only-to-write interest changes, size rejection and
pending idle-poll cancellation. Cover root and nested child ownership. These are finite
functional controls, not saturation or performance tests.

After a freshly authorized build, rerun the two unchanged v6 stdin differentials
against Node and the actual new immutable binary. Add direct async fd0 reads for
pipe data/EOF while IPC remains active, ordinary close(0) followed by IPC, IPC
disconnect followed by ordinary stdin data, nested forks, and stdout containing
the old control prefix. Retain exact errors/exits and owner cleanup evidence.
Run existing fork echo and stdin/child-process controls as bounded serial cases.
Only then reconsider application/broad comparison under a new resource window.

## Decisions for review

Frozen v8 proves piped stdin/IPC isolation, direct fd0 ownership, independent
disconnect lifetime, nested messages, callback context and descendant HTTP.
Ignored stdin still fails: the VFS-backed `/dev/null` positive-length read
returns `Some(empty)` while pipes report EOF as `None`. Stdin's synchronous
adapter treats the former as no readiness and its deferred adapter emits empty
data, so public stdin never ends and direct async read reports EAGAIN. The
isolated kernel correction normalizes only positive-length EOF in its ordinary
VFS/proc/anonymous read branches. Zero-length requests and the earlier socket,
pipe and PTY branches remain distinct. New owning controls cover those file
branches, zero-length requests, pipe/stream closure and empty datagrams; the
execution adapter also asserts its exact done response. No guest-side filtering
or IPC/stdin multiplexing is introduced. This successor is unbuilt.

- Use a kernel socket pair rather than two one-way pipes. It provides one duplex
  descriptor per process and existing socket accounting, at the cost of charging
  two sockets per channel. Do not raise limits to conceal that cost.
- Add a private deferred poll slot rather than generalizing all public descriptor
  waits. This confines the fix while preserving ordinary read/poll compatibility.
- Extend existing IPC queue bounds after readiness rather than add a new public
  tuning option. Pressure/failure semantics need the finite callback controls.
- Remove stdout framing as well as stdin framing. A one-direction repair would
  retain output corruption and would not establish independent IPC lifetime.
