---
title: "Sandbox"
description: "Mount a Sandbox Agent (Docker) filesystem into the VM and expose its process management as bindings."
category: "Reference"
order: 5
---

Back a VM with a real Sandbox Agent container: the sandbox's filesystem appears as a mount inside the VM, and its process management is callable as bindings. Reach for this when you want guest code to read, write, and run against a live Docker sandbox instead of the in-memory VFS.

## How it works

The embedded Core client passes `docker()` as the sandbox provider. The VM gets
its own Docker sandbox mounted under `/home/agentos/sandbox`, plus a `sandbox`
binding collection surfaced as the `agentos-sandbox` CLI command. These host
mounts and bindings are intentionally unavailable from the hosted Rust actor.
Disposing the VM destroys its sandbox.

## Run it

```sh
npm install
npx tsx client.ts
```

You should see `hello` printed from a file executed inside the Docker sandbox.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/sandbox
