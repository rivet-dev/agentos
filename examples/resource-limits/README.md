---
title: "Resource Limits"
description: "Configure VM resource limits, JavaScript CPU/wall-clock budgets, Python caps, and WASM runtime limits."
category: "Reference"
order: 4
---

Cap how much of the host a VM can consume. Reach for this when you run untrusted code and need hard ceilings on processes, file descriptors, sockets, filesystem storage, JavaScript CPU time, Python execution, and WASM runtime work.

## How it works

The fixed actor accepts a typed `limits` block in `createWithInput`. Kernel resources live under `limits.resources`; JavaScript, Python, and WASM runtime limits live under `limits.jsRuntime`, `limits.python`, and `limits.wasm`. The sidecar applies these during VM creation, so guest env vars cannot raise or override their own caps.

## Run it

```sh
npm install
npx tsx server.ts
```

This creates the actor VM with the configured resource caps and prints its runtime status.

## Source

View the source on GitHub: https://github.com/rivet-dev/agentos/tree/main/examples/resource-limits
