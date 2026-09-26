---
title: "Embedded VMs"
description: "Embedded agentOS runtime API."
category: "Reference"
order: 1
---

Use `@rivet-dev/agentos-core` to boot and control a VM directly from a trusted
Node.js application. The embedded API covers files, processes, terminals,
language execution, networking, command scheduling, software, permissions,
limits, host functions, and trusted host-backed mounts.

- `vm.ts`: the complete embedded runtime surface.
- `advanced.ts`: a dedicated sidecar process.
- `host-functions.ts`: expose a trusted host function to guest programs.
- `config-reference.ts`: common VM configuration.
- `limits.ts`: resource limits and warnings.
- `mounts.ts`: host-directory and S3 mounts.
- `persistence.ts`: restore filesystem state from SQLite.
- `permissions.ts`: configure the kernel permission policy.
- `software.ts`: install a local software package and run its command.

```sh
npm install
npx tsx vm.ts
```

- `wasm-backend.ts`: select Wasmtime for standalone WASM commands. The default backend remains V8.
