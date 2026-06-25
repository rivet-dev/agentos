---
title: "Approvals"
description: "Handle live permission requests with auto-approve and selective-approval flows."
category: "Sessions & Permissions"
order: 3
---

When an agent wants to read a file, write output, or run a command, the VM raises a permission request. This example shows how to handle those requests—either fully server-side (auto-approve) or by forwarding them to a client for a human-in-the-loop decision (selective approval). Reach for this when you need to control what an agent is allowed to do mid-session.

## How it works

Permissions flow through two complementary hooks:

- **Server-side (`onPermissionRequest`)**: a hook on `agentOS({ ... })` runs for every request before it reaches any client. Inspect `request.description` and `request.params` to approve, log, or filter requests in fully automated pipelines—no client round-trip needed.
- **Client-side (`permissionRequest` event)**: requests the server forwards reach the client over a live `agent.connect()` connection. The client decides and calls `agent.respondPermission(sessionId, permissionId, "once" | "reject")` to allow a single action or deny it.

The `selective` variants combine both: the server handles some requests itself and forwards the rest to the client. A local `pi` software fixture stands in for a real agent package so the example runs self-contained.

## Run it

```bash
npm install
# Auto-approve everything server-side:
npx tsx server.ts        # in one terminal
npx tsx client.ts        # in another

# Or run the auto-approve / selective variants:
npx tsx auto-approve.ts & npx tsx auto-approve-client.ts
npx tsx selective.ts     & npx tsx selective-client.ts
```

The agent runs its prompt and each permission request is approved, rejected, or logged according to the hook you chose.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/approvals
