---
title: "LLM Credentials"
description: "Pass LLM provider keys per session via env, including a per-tenant credential pattern."
category: "Sessions & Permissions"
order: 5
---

A VM never inherits the host `process.env`, so LLM provider keys must be handed to each session explicitly. Reach for this when your agent needs an `ANTHROPIC_API_KEY` (or any provider secret) and you want that key scoped to a single session — or to a single tenant — rather than baked into the server.

## How it works

The server declares the agent software but holds no credentials. The client passes keys through the `env` option on `createSession`, which injects them into that session's VM only. For multi-tenant setups, give each tenant an isolated VM keyed by their id and resolve their key from your own credential store at session-creation time. Keys live on the server and are never sent to the client.

## Run it

```bash
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # then, in another shell:
ANTHROPIC_API_KEY=sk-... npx tsx client.ts
```

The client prints a new session id; the agent inside the VM sees the key via its environment. See `per-tenant.ts` for the per-tenant variant.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/llm-credentials
