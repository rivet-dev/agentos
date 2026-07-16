---
title: "Networking"
description: "VM networking: loopback servers, fetch from inside and outside the VM, and signed preview URLs."
category: "Networking"
order: 1
---

# Networking

Run a service inside a VM and reach it — from the client and from the public web. Reach for this when an agent spins up a dev server, API, or web app that you need to call or share.

## How it works

A process inside the VM binds a normal loopback port (e.g. `3000`), exactly like any Node server. The client reaches it with `agent.vmFetch(port, path, options)`, which proxies an HTTP request straight to that loopback port without exposing it to the network. To expose a port beyond loopback, set `loopbackExemptPorts` on the VM config. For external sharing, `agent.createSignedPreviewUrl(port, expiresInSeconds)` mints a short-lived signed URL; the `preview` config sets default and maximum lifetimes plus a bounded maximum active-token count, and old tokens are removed automatically as they expire.

## Run it

```bash
npm install
# Start the VM host
npx tsx server.ts
# In another terminal, run a server in the VM and fetch it
npx tsx client-run-server.ts
npx tsx client-fetch.ts
```

Expect a `200` status and `Hello from inside the VM` printed by the fetch client.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/networking
