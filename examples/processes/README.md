---
title: "Processes"
description: "Process management inside the VM: exec, spawn, stdin, lifecycle, shell sessions, process events, and visibility."
category: "Processes & Shell"
order: 1
---

Run commands and long-lived processes inside a VM, stream their output, and drive interactive shells. Reach for this whenever an agent needs to invoke tools, start a dev server, pipe data over stdin, or attach a terminal.

## How it works

A `server.ts` registers a VM with its software, and each script connects with `createClient` and grabs a VM via `client.vm.getOrCreate("my-agent")`. From there the VM handle exposes the full process surface:

- **`exec`** — run a command to completion and collect `stdout`, `stderr`, and `exitCode` in one call.
- **`spawn` + lifecycle** — start a process for a `pid`, then `listProcesses`, `getProcess`, `waitProcess`, `stopProcess` (SIGTERM), and `killProcess` (SIGKILL).
- **stdin** — `writeProcessStdin` and `closeProcessStdin` to feed a running process, including an interactive `sh` (see `shell.ts`).
- **subscriptions** — call `agent.connect()` and use `onProcessOutput`, `onProcessExit`, `onShellData`, `onShellStderr`, and `onShellExit`. The connection filters RivetKit broadcasts to the requested process or shell.

`visibility.ts` shows how to enumerate and inspect everything running in the VM.

## Run it

```bash
npm install
npx tsx server.ts &   # start the VM registry on :6420
npx tsx exec.ts       # then run any of the scripts (spawn.ts, stdin.ts, shell.ts, ...)
```

Each script prints its process output and exit codes to the console.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/processes
