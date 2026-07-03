---
title: "Sandbox"
description: "Give an agent a Docker-backed environment for native tools."
category: "Quickstart"
order: 11
---

Back a VM with a Docker-backed environment so an agent can reach native tools when the in-process runtime is not enough. Reach for this when you want the agent to solve tasks that may need a familiar Linux filesystem, package manager, or shell.

## How it works

The quickstart passes `sandbox: { provider: docker() }` to `AgentOs.create`, then asks an agent to write, compile, and run a C program. Agent OS mounts the container filesystem, registers process bindings, and disposes the sandbox client when the VM is disposed. The prompt does not mention the sandbox; the agent sees the available environment and chooses how to complete the task. Set `SKIP_DOCKER=1` to no-op the example where Docker is unavailable.

## Run it

```bash
npm install
npx tsx index.ts
```

You should see streamed agent events and a final response containing the compiled program's output.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/sandbox
