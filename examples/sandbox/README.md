---
title: "Sandbox"
description: "Give each VM a Docker-backed environment for native tools."
category: "Reference"
order: 5
---

Back each VM with its own real Sandbox Agent container so agents can reach native tools when the in-process runtime is not enough.

## How it works

The server passes `sandbox: { provider: docker() }` directly to `agentOS(...)`. The provider is a factory, so every actor VM gets its own sandbox client/container and Agent OS disposes it when the actor VM sleeps or is destroyed. The client asks an agent to write, compile, and run a C program without naming the sandbox; the agent uses the available environment to complete the task.

## Run it

```sh
npm install
tsx server.ts
tsx client.ts
```

You should see streamed agent events and a final response containing the compiled program's output.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/sandbox
