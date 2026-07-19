# Filesystem

Read, write, mount, and manage files inside agentOS, all backed by a virtual filesystem isolated from the host disk.

Each VM has its own filesystem that the agent works in. Guest `fs` calls never touch the host disk, and it persists automatically across sleep/wake with no setup. See [Persistence](/docs/persistence) for the details.

## Mounts

Back a guest path with external storage by adding it to the `mounts` config. Each mount takes a `path` and an optional `readOnly` flag, and the guest only ever sees the mounted subtree, never the wider host.

Project a real host directory into the filesystem, Docker-style. The guest sees only the mounted subtree, never the wider host filesystem. Path-escape attempts (symlinks, `..`, path aliasing) are confined to the mount root.

Mount an S3 bucket with the built-in `s3` plugin. Pass an optional `prefix` to scope storage to a key path within the bucket, useful for sharing one bucket across multiple agents.

The backend is a block store, not a one-object-per-file mapping: file contents are split into fixed-size chunks (4 MB by default) stored as individual S3 objects, with a separate metadata layer mapping each file to its chunks. This keeps large files, partial reads and writes, and snapshots efficient without rewriting whole objects.

The `s3` plugin config also accepts `credentials` (`{ accessKeyId, secretAccessKey }`) and a custom `endpoint` for S3-compatible providers.

Mount a Google Drive folder with the built-in `google_drive` plugin.

Use the built-in `memory` plugin for an ephemeral mounted directory in the native RivetKit `agentOS()` actor.

Use `mountFs()` for a callback-backed JS filesystem driver. The driver must live in the same JS process as the `AgentOs` instance, such as direct core usage or a custom RivetKit actor that owns an `AgentOs` instance. `mountFs()` works on a running VM too — it returns a promise that resolves once the mount is visible to guest code, and rejects if delivery to the runtime fails.

The native `agentOS()` actor cannot accept `{ driver }` mounts in config because JS callback objects are not serializable across the native plugin boundary. Use `plugin` mounts there.

## File operations

These operations are primarily what the agent uses inside the VM, and are also available from the client to seed inputs and read results. For large or read-only inputs (a repo, a dataset), a read-only [host mount](#mounts) is faster than copying files in. Programs that need stdin or live output use exec instead (see [Core](/docs/core)).

### Read and write

### Batch read and write

### Directories

### File metadata

### Move and delete

## Change notifications

The runtime reports guest filesystem changes to the host, coalesced per VM: every successful mutation (host API writes, guest `fs.*` calls, shell command output files, Python VFS writes) marks its parent directory, and at most every ~300ms the VM flushes one event listing the directories whose entries changed. Pass `onFsChanged` when creating the VM:

```ts
const vm = await AgentOs.create({
	onFsChanged: ({ dirs, overflow }) => {
		if (overflow) return refreshEverything();
		for (const dir of dirs) refreshListing(dir);
	},
});
```

The Rust client exposes the same stream as `AgentOs::on_fs_changed()`. Two bounds keep the signal cheap and truthful: a window holds at most 64 distinct directories (`fs_changed_dirty_dirs`, warned near capacity like every other tracked limit), and past the bound the event collapses to `overflow: true`, meaning "treat the whole tree as changed" — changes are never dropped silently. A subscriber that falls behind receives the same overflow marker instead of missing windows. The inspector's Filesystem tab updates in realtime from exactly this event.

## Permissions

Filesystem access is governed by the VM permission policy. The filesystem scope is granted by default; restrict it by path, for example to deny a sensitive directory:

```ts
const vm = agentOS({
  permissions: {
    fs: {
      default: "allow",
      rules: [{ mode: "deny", operations: ["*"], paths: ["/home/agentos/secrets/**"] }],
    },
  },
});
```

See [Permissions](/docs/permissions) for the full configuration.

## Sandboxes

For heavier or untrusted workloads, run a full Linux [sandbox](/docs/sandbox) alongside the VM and mount its filesystem into agentOS. The agent then reads and writes the sandbox's files through the same `fs` APIs while the sandbox handles execution. See [Sandbox Mounting](/docs/sandbox) for setup.

## Default layout

With no `mounts` configured, every VM boots an Alpine-based root filesystem with the standard POSIX directories:

- `/home/agentos`: the agent's home directory (`$HOME`) and default working directory (`pwd`) when spawned, where it reads and writes (mounts land under it, e.g. `/home/agentos/data`).
- `/bin`, `/sbin`, `/usr`: installed commands (common POSIX utilities by default, plus any [software](/docs/software) you add).
- `/etc`, `/lib`, `/opt`, `/root`, `/run`, `/srv`, `/tmp`, `/var`, `/mnt`: standard system paths.

It is backed by the VM's own filesystem and persisted across sleep/wake. Nothing comes from or touches the host disk.