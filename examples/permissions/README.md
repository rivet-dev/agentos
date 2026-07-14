---
title: "Permissions"
description: "Apply permission policies: grant network, deny filesystem paths, and scope what the guest can do."
category: "Sessions & Permissions"
order: 2
---

Permission policies decide what guest code is allowed to touch — the network, the filesystem, and named bindings. Reach for this when you need to hand untrusted or agent-generated code a VM that can only do exactly what you intend.

## How it works

Each policy is a small object passed to `agentOS({ permissions })`. A policy sets a `default` (`allow` or `deny`) and a list of `rules` that flip the decision for specific paths, hosts, or binding names. This example composes three restrictive policies and merges them into one permission set:

- **Network** uses an explicit rule set whose local default is deny and allows only `api.example.com`.
- **Filesystem** allowed by default but denied for anything under `/vault/**`.
- **Bindings** use an explicit rule set whose local default is deny, allowing only the `add` binding by name.

Rules are evaluated against the defaults, so you compose from broad posture down to narrow exceptions. The resulting VM enforces all of them on every guest operation.

## Run it

```sh
npm install
npx tsx server.ts
```

The registry starts with a VM whose guest can reach `api.example.com`, cannot read `/vault`, and can only invoke the `add` binding.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/permissions
