---
title: "Permissions"
description: "Apply permission policies that scope what guest code can do."
category: "Configuration"
order: 2
---

Permission policies decide what guest code is allowed to touch, including the network and filesystem. Reach for this when you need to hand untrusted code a VM that can only do exactly what you intend.

## How it works

Each policy is passed in the actor creation config. A policy sets a `default` (`allow` or `deny`) and a list of `rules` that flip the decision for specific paths or hosts. This example composes policies into one permission set:

- **Network** granted outright, with a stricter override that denies by default and allows only `api.example.com`.
- **Filesystem** allowed by default but denied for anything under `/vault/**`.

Host bindings are available only when embedding agentOS Core. They are not part
of the hosted actor API.

Rules are evaluated against the defaults, so you compose from broad posture down to narrow exceptions. The resulting VM enforces all of them on every guest operation.

## Run it

```sh
npm install
npx tsx server.ts
```

The client creates a VM whose guest can reach `api.example.com` and cannot read `/vault`.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/permissions
