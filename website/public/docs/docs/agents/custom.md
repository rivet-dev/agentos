# Custom Agents

Bring your own coding agent to agentOS by speaking the Agent Client Protocol (ACP) inside the VM.

A custom agent is a program that runs **inside the VM** to drive a coding agent. agentOS spawns it when you call `createSession()` and talks to it over the Agent Client Protocol. You ship it as a software package, exactly like the built-in agents.

## Agent Client Protocol (ACP)

agentOS speaks the [Agent Client Protocol (ACP)](https://agentclientprotocol.com) to every agent: JSON-RPC over stdio. The agent reads protocol messages on **stdin** and writes them on **stdout**, so stdout is reserved for ACP and **stderr is used for logs**. Your program only needs to speak ACP; how it runs the underlying model is up to you. See the [ACP documentation](https://agentclientprotocol.com) for the full protocol.

## Two ways to build an agent

There are two shapes, depending on whether the agent runs in the ACP process or in its own.

### Single process (embedded)

The ACP adapter **embeds the agent SDK** and runs it in the same process. One process inside the VM, lower memory footprint.

  <text x="170" y="36" text-anchor="middle" font-size="11" fill="#1b1916">Host</text>
  <text x="184" y="70" font-size="10" fill="#56524a">ACP</text>
  <text x="64" y="110" font-size="11" fill="#56524a">VM</text>
  <text x="170" y="163" text-anchor="middle" font-size="11" fill="#1b1916">ACP adapter +</text>
  <text x="170" y="180" text-anchor="middle" font-size="11" fill="#1b1916">agent (embedded)</text>

For example, an adapter to run **OpenCode**, which speaks ACP natively. One package is both the ACP process and the agent, so there's no separate adapter and nothing else is spawned.

### ACP adapter (separate agent)

The ACP adapter is a thin **bridge** that spawns the real agent as its **own process** (a CLI or SDK) and translates between it and ACP. Full agent feature set, higher memory.

  <text x="170" y="36" text-anchor="middle" font-size="11" fill="#1b1916">Host</text>
  <text x="184" y="70" font-size="10" fill="#56524a">ACP</text>
  <text x="64" y="110" font-size="11" fill="#56524a">VM</text>
  <text x="170" y="144" text-anchor="middle" font-size="11" fill="#1b1916">ACP adapter</text>
  <text x="184" y="174" font-size="10" fill="#56524a">spawns</text>
  <text x="170" y="202" text-anchor="middle" font-size="11" fill="#1b1916">Agent process</text>
  <text x="170" y="217" text-anchor="middle" font-size="10" fill="#56524a">(CLI / SDK)</text>

For example, an adapter to run **Pi**: the `pi` CLI doesn't speak ACP, so `pi-acp` speaks ACP and spawns the CLI as a separate process. The packaged agent bundles both — its `agentos-package.json` names `pi-acp` as the `acpEntrypoint` and points it at the `pi` CLI via `agent.env`.

## Use your agent

Register the package on the server with `software`. Sessions are then created from the client by `id`, exactly like any built-in agent.

```ts title="server.ts"
import { agentOS, setup, defineSoftware } from "@rivet-dev/agentos";

const myAgent = defineSoftware({
  packageDir,   // the packaged agent directory; its agentos-package.json carries the agent block
});

const vm = agentOS({ software: [myAgent] });

export const registry = setup({ use: { vm } });
registry.start();
```

See [Sessions](/docs/sessions) for creating and driving sessions. Package your adapter with `agentos-toolchain pack --agent my-agent-acp` so its dependencies are bundled into the self-contained package directory and the `agent` block (naming the `bin/` ACP entrypoint) is written into the package's `agentos-package.json`, rather than shipping it as a loose file.

All built-in agents are defined exactly this way. Browse them for reference on [GitHub](https://github.com/rivet-dev/agentos/tree/main/software).

## Read more

- [Defining software packages](/docs/custom-software/definition): the full descriptor reference, including the `agentos-package.json` schema and every `agent` field (`acpEntrypoint`, `env`, `launchArgs`, `snapshot`).
- [Building binaries](/docs/custom-software/building-wasm): compile WASM command binaries and use the registry.

## Debugging

When a custom agent exits mid-turn or a tool call fails, capture the agent's stderr with the `onAgentStderr` hook on `AgentOs.create()`. The agent uses stdout for ACP, so stderr carries its logs and crash output. See [Debugging](/docs/debugging) for that hook and the runtime (sidecar) logs.
