---
title: "Approvals"
description: "Handle live permission requests with auto-approve and selective-approval flows."
category: "Sessions & Permissions"
order: 3
---

When an agent asks to use a tool, its ACP adapter can raise a permission request. This example shows unattended `allow_all` and interactive `ask` policies. ACP decisions are advisory; VM permissions remain the runtime security boundary.

## How it works

Permissions use the ordinary durable session-event stream:

- **Automatic**: omit `permissionPolicy` or set `allow_all`. AgentOS deterministically selects an adapter-supplied allow option without emitting or persisting the request.
- **Interactive**: set `permissionPolicy: "ask"`, consume `permission_request` variants from `sessionEvent`, and call `respondPermission({ sessionId, requestId, optionId })` with an exact offered option ID.

Subscribing to `sessionEvent` does not enable interactive approval. The policy is fixed when the session opens; if `permissionPolicy` is omitted, the default `allow_all` policy resolves the adapter request automatically and no `permission_request` event is emitted.

The `selective` client inspects the exact native tool call and chooses an offered allow or reject option. Requests have no expiry; they remain pending until answered or terminated by prompt/session/adapter/VM lifecycle. A local `pi` software fixture stands in for a real agent package so the example runs self-contained.

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

The agent runs its prompt and each interactive request is recoverable from durable session history. Reconnecting consumers merge history with live events and deduplicate by `(sessionId, sequence)`.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/approvals
