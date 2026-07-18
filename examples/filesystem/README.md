---
title: "Filesystem"
description: "Filesystem access: host-side file APIs, VFS isolation, and mounting memory, host directories, S3, and Google Drive."
category: "Filesystem"
order: 1
---

Every VM gets an isolated virtual filesystem (VFS) that you drive from the host. Reach for this when you need to seed files into a VM, read results back out, or expose external storage to the guest without leaking the host disk.

## How it works

The host client exposes file APIs directly on a VM handle — `writeFile`/`readFile` for single files, `writeFiles`/`readFiles` for batches, plus `mkdir`, `readdir`, `readdirRecursive`, `stat`, `exists`, `move`, and `remove`. Bytes you write land in the kernel's in-memory VFS, which the guest sees through the normal `node:fs` API; the real host disk is never exposed, so a path that exists in the VFS does not exist on the host.

To bridge in external storage, declare `mounts` on `agentOS({ ... })`. Each mount maps a guest path to a plugin: `memory` for scratch space, `host_dir` for a host directory (optionally `readOnly`), `s3` for a bucket/prefix, or `google_drive` for a Drive folder. The guest reads and writes those paths like any other directory.

## Run it

```bash
npm install
npx tsx server.ts          # start the VM host
npx tsx operations.ts      # in another shell: exercise the file APIs
```

`operations.ts` writes, reads, lists, moves, and deletes files; `isolation.ts` shows the VFS is sealed from the host disk; the `mount-*.ts` servers swap in different storage backends.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/filesystem
