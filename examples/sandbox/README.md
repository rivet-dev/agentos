---
title: "Sandbox"
description: "Mount a fresh Sandbox Agent (Docker) filesystem into each VM and expose its process management as bindings."
category: "Reference"
order: 5
---

Back each VM with its own real Sandbox Agent container: the sandbox's filesystem appears as a mount inside the VM, and its process management is callable through sandbox bindings. Reach for this when you want guest code to read, write, and run against a live Docker sandbox instead of the in-memory VFS.

## How it works

The helper starts one sandbox for one VM, then wires it in two ways. `createSandboxFs({ client })` returns a mount-plugin descriptor that projects the sandbox filesystem under `/home/agentos/sandbox`, so `vm.writeFile` and `vm.exec` operate on real container files. `createSandboxBindings({ client })` exposes the sandbox's process management as the `agentos-sandbox` CLI command. The client disposes both the VM and sandbox together.

Do not create one `SandboxAgent` at module scope and reuse it for multiple actor instances. Dynamic per-actor sandbox creation for `agentOS(...)` needs a future actor-scoped options hook; no `createOptions` callback is supported today.

## Run it

```sh
npm install
tsx client.ts
```

You should see text read back from a file written through the sandbox mount.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/sandbox
