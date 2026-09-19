---
title: "Sandbox"
description: "Mount a Docker sandbox filesystem and run commands through sandbox host functions."
category: "Quickstart"
order: 11
---

Back a VM with a Docker-backed sandbox so guest reads, writes, and commands run inside a real container. Reach for this when you want host-isolated execution with a familiar filesystem and shell, instead of the in-process runtime.

## How it works

The `docker()` provider starts one Docker container for the VM. agentOS mounts its filesystem at `/mnt/sandbox`, registers a `sandbox` host-function collection for running commands, and destroys the container when the VM is disposed. Set `SKIP_DOCKER=1` to skip the example when Docker is unavailable.

## Run it

```bash
npm install
npx tsx index.ts
```

You should see a file read back from the sandbox mount, the host-function RPC port, and the output of an `echo` command plus a process listing from inside the Docker sandbox.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/sandbox
