# Skyvern example

Drive a browser from an agentOS VM through Skyvern's hosted MCP server. The browser runs in Skyvern Cloud. The VM only makes HTTPS calls to `api.skyvern.com`.

`server.ts` defines a VM with the Claude Code agent and a network policy that allows only `api.anthropic.com` and `api.skyvern.com`. `client.ts` opens a session with Skyvern's MCP server in `mcpServers`, then asks the agent to open a page, click a link, and report the title of the new page. `embedded.ts` runs a similar flow with the embedded API.

## Setup

Copy an API key from Settings in the [Skyvern dashboard](https://app.skyvern.com/settings), then:

```bash
export SKYVERN_API_KEY=...
export ANTHROPIC_API_KEY=sk-ant-...
```

## Run

```bash
pnpm start          # start the agentOS server (server.ts)
pnpm client         # open a session and let the agent drive the browser
pnpm embedded       # same flow with the embedded API, no server needed
```
