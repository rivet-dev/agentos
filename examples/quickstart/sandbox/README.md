---
title: "Sandbox"
description: "Mount a Docker sandbox filesystem and run commands through sandbox bindings."
category: "Quickstart"
order: 11
---

Back a VM with a Docker-backed sandbox so guest reads, writes, and commands run inside a real container. Reach for this when you want host-isolated execution with a familiar filesystem and shell, instead of the in-process runtime.

## How it works

A `SandboxAgent` starts a Docker container via `sandbox-agent`. Two pieces wire it into the VM: `createSandboxFs` mounts the container's filesystem at `/sandbox`, and `createSandboxBindings` registers a `sandbox` binding collection for running commands. Files written under `/sandbox` land in the container, while bindings such as `run-command` and `list-processes` execute against it through generated `agentos-sandbox` CLI commands. Set `SKIP_DOCKER=1` to no-op the example where Docker is unavailable.

## Run it

```bash
npm install
npx tsx index.ts
```

You should see a file read back from the sandbox mount, the bindings RPC port, and the output of an `echo` command plus a process listing from inside the Docker sandbox.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/sandbox
