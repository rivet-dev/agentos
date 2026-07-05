---
title: "Resource Limits"
description: "Configure VM resource limits, JavaScript CPU/wall-clock budgets, Python caps, and WASM runtime limits."
category: "Reference"
order: 4
---

Cap how much of the host a VM can consume. Reach for this when you run untrusted or agent-generated code and need hard ceilings on processes, file descriptors, sockets, filesystem storage, JavaScript CPU time, Python execution, and WASM runtime work.

## How it works

The VM accepts a typed `limits` block when you call `agentOS({ ... })`. Kernel resources live under `limits.resources`; JavaScript, Python, and WASM runtime limits live under `limits.jsRuntime`, `limits.python`, and `limits.wasm`. The sidecar forwards these over the VM creation wire, so guest env vars cannot raise or override its own caps.

## Run it

```sh
npm install
npx tsx server.ts
```

This starts a registry whose VM is provisioned with the configured resource caps.

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/resource-limits
