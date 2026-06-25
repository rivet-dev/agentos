---
title: "Resource Limits"
description: "Configure VM resource limits: processes, file descriptors, sockets, filesystem bytes, and WASM stack."
category: "Reference"
order: 4
---

Cap how much of the host a VM can consume. Reach for this when you run untrusted or agent-generated code and need hard ceilings on processes, file descriptors, sockets, filesystem storage, and WASM stack depth.

## How it works

The VM accepts a `limits.resources` block when you call `agentOS({ ... })`. Each field bounds one kind of resource the guest can hold open at once: `maxProcesses`, `maxOpenFds`, `maxSockets`, a `maxFilesystemBytes` storage budget for the VFS, and a `maxWasmStackBytes` ceiling on the WASM call stack. The sidecar enforces these against the executor, so a guest that tries to exceed a cap is denied rather than allowed to exhaust the shared process. Defaults are bounded already; these values raise or lower them to fit your workload.

## Run it

```sh
npm install
npx tsx server.ts
```

This starts a registry whose VM is provisioned with the configured resource caps.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/resource-limits
