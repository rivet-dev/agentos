---
title: "Core"
description: "Core AgentOs API: exec, config reference, lifecycle hooks, and mounts."
category: "Reference"
order: 1
---

The core AgentOs API surface in one place: define a VM server, connect a typed client, and drive VMs for exec, filesystem, processes, agent sessions, networking, and cron. Reach for this when you want a reference of what a `handle` can do and how the server is configured.

## How it works

`agentOS({ ... })` defines a VM with its mounts, software, lifecycle hooks, and preview/network settings, then `setup({ use: { vm } }).start()` exposes it over the wire. On the client, `createClient<typeof registry>()` gives a typed `client`, and `client.vm.getOrCreate(id)` returns a `handle`. Everything runs through that handle: `exec`/`spawn` for processes, `readFile`/`writeFiles`/`readdirRecursive` for the filesystem, `createSession`/`sendPrompt` for agents, `openShell` for interactive terminals, `vmFetch` for in-VM servers, and `scheduleCron` for jobs. `handle.connect()` opens an event stream for process output, session events, permission requests, and cron events.

- `server.ts` / `config-reference.ts` — VM definition and the full config surface (mounts, software, loopback exemptions, preview token lifetimes, hooks).
- `hooks.ts` — server-side lifecycle hooks like `onSessionEvent`.
- `mounts.ts` — host-directory and S3 mount descriptors.
- `client.ts` — every client capability against a `handle`.

## Run it

```sh
npm install
npx tsx server.ts   # start the VM server, then run client.ts against it
```

The server listens on `http://localhost:6420`; the client connects, creates a VM, and exercises exec, filesystem, sessions, and more.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/core
