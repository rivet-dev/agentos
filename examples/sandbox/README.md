---
title: "Sandbox"
description: "Mount a Sandbox Agent (Docker) filesystem into the VM and expose its process management as bindings."
category: "Reference"
order: 5
---

Back a VM with a real Sandbox Agent container: the sandbox's filesystem appears as a mount inside the VM, and its process management is callable as bindings. Reach for this when you want guest code to read, write, and run against a live Docker sandbox instead of the in-memory VFS.

## How it works

The server starts a sandbox through `SandboxAgent.start({ sandbox: docker() })`, then wires it into `agentOS` two ways. `createSandboxFs({ client })` returns a mount-plugin descriptor that projects the sandbox filesystem under `/home/agentos/sandbox`, so `vm.writeFile` and `vm.exec` operate on real container files. `createSandboxBindings({ client })` exposes the sandbox's process management as bindings, surfaced inside the VM as the `agentos-sandbox` CLI command. From the client you write a file to the mount, `exec` it, invoke a binding like `run-command`, and `spawn` a long-running process whose stdout/stderr stream back over `vm.connect()`.

## Run it

```sh
npm install
npm run server   # starts the VM with the sandbox mount + bindings
npm run client   # writes a file, runs it, and streams process output
```

You should see `hello` printed from a file executed inside the Docker sandbox, followed by streamed output from the spawned dev process.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/sandbox
