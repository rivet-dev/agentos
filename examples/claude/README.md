---
title: "Claude Agent"
description: "Run the Claude Code agent in a session using an Anthropic API key."
category: "Agents"
order: 2
---

Run the Claude Code agent inside a VM session and drive it with prompts. Reach for this when you want a coding agent that reads, writes, and runs commands in an isolated environment instead of calling the model API directly.

## How it works

The server registers a VM with the Claude Code software and starts the registry. The client calls `openSession({ agent: "claude", env: … })`, which infers the default `main` session, passes `ANTHROPIC_API_KEY` through the session env, then calls `prompt({ content })` to get the agent's response. From there you can layer on extras: drop a `SKILL.md` into `~/.claude/skills/` before opening the session and the agent discovers it automatically, or configure MCP servers in Claude Code's native `~/.claude.json` file. Pre-install local MCP servers with `exec` so first-run `npx` output does not corrupt the stdio handshake.

## Run it

```sh
npm install
ANTHROPIC_API_KEY=sk-ant-... npm run start
```

The agent answers the prompt and prints its response to the console.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/claude
