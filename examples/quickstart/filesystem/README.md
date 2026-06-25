---
title: "Filesystem"
description: "Filesystem operations: write, read, mkdir, readdir, stat, move, and delete."
category: "Quickstart"
order: 4
---

Work with files and directories inside a VM. Reach for this when your guest code needs to lay out a directory tree, inspect entries, or move and clean up files before handing results back.

## How it works

Every VM boots with an in-memory filesystem, so no setup is required. The `AgentOs` instance exposes the full set of POSIX-style operations directly: `mkdir`, `writeFile`, `readdir` (and `readdirRecursive` with a `maxDepth`), `stat`, `exists`, `move`, and `delete` (with `{ recursive: true }`). For durable or external storage, pass `mounts` at boot — for example an S3 backend mounted at `/data` — and the same operations apply transparently to those paths.

## Run it

```bash
npm install
npx tsx index.ts
```

You should see the project tree listed, file stats printed, and existence checks flip to `false` as files are moved and deleted.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/filesystem
