---
title: "Authentication"
description: "Validate client credentials server-side with onBeforeConnect connection hooks."
category: "Sessions & Permissions"
order: 4
---

# Authentication

Gate access to your VMs by validating client credentials on the server before any connection is established. Reach for this whenever clients must present a token (a JWT, API key, or session ID) that you verify before letting them create sessions or send prompts.

## How it works

The client passes credentials as connection `params` when it calls `getOrCreate`. Those params are forwarded to the server, where an `onBeforeConnect` hook inspects them and rejects the connection by throwing. Because `params` is typed as `unknown` on the wire, the hook is the real enforcement point: it checks the token's shape and validity (signature, lookup, expiry) and either returns to admit the connection or throws to deny it. Once admitted, every action on the handle runs against that authenticated connection.

## Run it

```bash
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # in one terminal
ANTHROPIC_API_KEY=sk-... npx tsx client.ts   # in another
```

A client with a valid `authToken` connects and lists the working directory; one with a missing or empty token is rejected before any session is created.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/authentication
