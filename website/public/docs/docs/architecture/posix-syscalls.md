# POSIX Syscalls

How agentOS extends WASI in two layers so WebAssembly guests behave like normal POSIX programs on top of the kernel.

<Note>These internal architecture docs are mostly generated and maintained by LLMs, then reviewed by humans. They are intentionally verbose; use your preferred LLM to ask focused questions about the architecture as needed.</Note>

Not everything inside an agentOS VM is JavaScript. The shell (`sh`) and the
coreutils behind [process execution](/docs/processes) ship as WebAssembly
binaries, and you can run your own WASM programs too. To make those programs
behave like normal Linux tools, agentOS presents a POSIX syscall surface on top
of WebAssembly.

- **WASM is a first-class guest.** WASM binaries run beside JavaScript inside the same VM.
- **Same kernel, same boundary.** WASM syscalls route through the same kernel that backs JS guests, so there is no extra host access.
- **POSIX shape, not host access.** The extensions below add process, user, and network *semantics*, all virtualized.

## Why WASI alone is not enough

The base standard for WASM system access is **WASI** (specifically `wasip1`).
WASI is intentionally minimal:

- It gives a guest preopened file descriptors, clocks, randomness, and basic file I/O.
- It has **no process model** (no `fork` / `exec` / `wait`).
- It has **no users or groups** (no `getuid` / `getgid`).
- It has **no general sockets** (no `connect` / `listen`).

Real command-line programs expect all of those. agentOS closes the gap in two
layers, and both route through the kernel rather than the host.

Every WASM syscall, like every JS syscall, goes through the kernel-owned virtual
filesystem, process table, and socket table. The extensions below add POSIX
*shape*; they do not add host access. See the [Security Model](/docs/security-model)
for the isolation boundary.

## The two-layer model

agentOS layers a POSIX surface over WASM. Layer 1 adds capabilities WASI does
not express at all; Layer 2 adapts the standard WASI calls so a normal libc
behaves correctly inside the VM. Both bottom out in the kernel.

  <text x="350" y="44" text-anchor="middle" font-size="15" font-weight="600" fill="#18181b">WASM guest (sh, coreutils, your .wasm)</text>
  <text x="350" y="64" text-anchor="middle" font-size="12" fill="#52525b">compiled for wasm32-wasip1, linked against patched wasi-libc</text>

  <text x="190" y="124" text-anchor="middle" font-size="14" font-weight="600" fill="#3730a3">Layer 1: host import modules</text>
  <text x="190" y="148" text-anchor="middle" font-size="12" fill="#3730a3">host_process &mdash; spawn / wait</text>
  <text x="190" y="168" text-anchor="middle" font-size="12" fill="#3730a3">host_user &mdash; uid / gid</text>
  <text x="190" y="188" text-anchor="middle" font-size="12" fill="#3730a3">host_net &mdash; TCP sockets</text>
  <text x="190" y="208" text-anchor="middle" font-size="12" fill="#3730a3">host_sleep_ms &mdash; blocking sleep</text>

  <text x="510" y="124" text-anchor="middle" font-size="14" font-weight="600" fill="#065f46">Layer 2: kernel-backed WASI shim</text>
  <text x="510" y="148" text-anchor="middle" font-size="12" fill="#065f46">stdio through the kernel bridge</text>
  <text x="510" y="168" text-anchor="middle" font-size="12" fill="#065f46">mounts mirrored as preopens</text>
  <text x="510" y="188" text-anchor="middle" font-size="12" fill="#065f46">read-only tiers enforced</text>
  <text x="510" y="208" text-anchor="middle" font-size="12" fill="#065f46">paths confined to their mount</text>

  <text x="350" y="308" text-anchor="middle" font-size="15" font-weight="600" fill="#18181b">Kernel: virtual filesystem, process table, socket table</text>
  <text x="350" y="328" text-anchor="middle" font-size="12" fill="#52525b">same paths that back JavaScript guests &mdash; no host escape</text>

## Layer 1: custom host import modules

Standard WASI cannot express `fork` / `exec`, `getuid`, or `connect`. agentOS
declares extra WebAssembly import modules that the host runtime implements, so
guest libc can call them as if they were ordinary syscalls. These bindings live
in the `wasi-ext` crate and cover three areas:

- **`host_process`**: process management. Spawn a child process (argv, env, inherited stdio fds, working directory), wait for a child to exit, and related file-descriptor operations. This is what gives a WASM `sh` real [child process](/docs/processes) semantics; spawns go through the kernel process table.
- **`host_user`**: user and group identity (uid, gid, user info). Base WASI has no concept of a user; this lets tools that call `getuid` / `getgid` see the VM's virtualized identity.
- **`host_net`**: TCP sockets (connect, listen, send, receive) through the kernel socket table, gated by the same [network permission policy](/docs/networking) as everything else. Base WASI has no general socket API.

A small `host_sleep_ms` binding provides blocking sleep. Together these let a
guest compiled for `wasip1` behave as if it had a process model, user identity,
and a network, all virtualized.

```c
// Imported from the host runtime, declared by the wasi-ext bindings.
// Guest libc calls these as if they were ordinary syscalls.
__attribute__((import_module("host_process"), import_name("proc_spawn")))
int host_proc_spawn(const char *argv, const char *envp, int cwd_fd);

// getuid returns an errno; the uid is written through the out-pointer.
__attribute__((import_module("host_user"), import_name("getuid")))
int host_getuid(unsigned int *ret_uid);

__attribute__((import_module("host_net"), import_name("net_connect")))
int host_net_connect(int fd, const char *addr, int addr_len);
```

## Layer 2: the kernel-backed WASI shim

The second layer adapts the standard WASI calls themselves so that programs
built against a normal libc behave correctly inside the VM. The embedded shim:

- **Routes stdio through the kernel.** `fd_read` / `fd_write` on the standard descriptors go through the kernel stdio bridge rather than host file descriptors, so output stays inside the VM and honors PTYs and redirection.
- **Fills in libc expectations.** For example `fcntl(F_SETFL)` is serviced via `fd_fdstat_set_flags`, so flag changes that libc performs do not fail.
- **Mirrors mounts as preopens.** The preopen table reflects the VM's guest path mappings, so mounted directories are visible to WASM path resolution exactly as they are to JS and to `node:fs`.
- **Enforces read-only tiers.** `path_open` rejects create / truncate / write flags on read-only mounts while still allowing non-mutating opens (directory traversal, `O_DIRECTORY`), so read-only mounts stay read-only without breaking `find`, `ls`, and friends.
- **Confines paths to their mount.** Targets are resolved beneath the specific preopen's root, so `..` segments cannot escape one mount into a sibling mount or a host path.

```
fd_read(0)            -> kernel stdio bridge   (not a host fd)
fcntl(fd, F_SETFL)    -> fd_fdstat_set_flags   (libc flag changes succeed)
path_open("/data/x")  -> resolved under the /data preopen root
path_open(..O_CREAT)  -> rejected on a read-only mount
path_open("../../etc")-> stays inside the mount; cannot escape
```