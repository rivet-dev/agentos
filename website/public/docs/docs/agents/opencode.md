# OpenCode

Run the OpenCode coding agent inside a VM with skills, MCP servers, and custom configuration.

## Quick start

Read [Sessions](/docs/sessions) first for session options, streaming events, prompts, and lifecycle management.

## LLM Credentials

OpenCode auto-detects a provider when its key is present on the session's `env`, sourced from your server's environment. Common variables:

- `ANTHROPIC_API_KEY` — Anthropic (Claude), the default.
- `OPENAI_API_KEY` — OpenAI.
- `OPENROUTER_API_KEY` — OpenRouter.
- `GEMINI_API_KEY` — Google Gemini.
- `GROQ_API_KEY` — Groq.
- …plus Amazon Bedrock, Azure, Google Vertex, and 70+ providers via [models.dev](https://models.dev).

See [LLM Credentials](/docs/llm-credentials), and OpenCode's [providers docs](https://opencode.ai/docs/providers/) for the full list.

## Skills

OpenCode discovers `SKILL.md` files from its skills directory. Write the skill into the VM before creating a session and OpenCode loads it automatically.

## MCP servers

Expose extra tools to the agent by passing `mcpServers` to `createSession`. Both local child-process servers and remote URLs are supported.

**Pre-install `npx`-launched servers.** A local server started with `npx -y …` writes install progress to **stdout** on its first run, which corrupts the MCP stdio handshake (you'll see `Connection closed`). Pre-install it in the VM so `npx` is silent — `await agent.exec("npm install -g @modelcontextprotocol/server-filesystem")` before the session — or pin the package and point `command` at the installed binary.

## Customizing the agent

OpenCode is a built-in agent, but it's just a software package under the hood. To ship your own ACP adapter, swap the underlying agent SDK, or register a tweaked build as a new agent, see [Custom Agents](/docs/agents/custom).