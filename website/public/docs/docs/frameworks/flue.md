# Flue

Use agentOS as the durable sandbox backend for Flue.

Flue owns the agent runtime and session lifecycle. Rivet maps each agent instance and workflow run to a durable Rivet Actor, while agentOS gives each Flue context an isolated VM with a persistent `/workspace` filesystem.

[View the complete example →](https://github.com/rivet-dev/agentos/tree/main/examples/flue)

## Quickstart

```sh
mkdir my-agent && cd my-agent
npm add @flue/runtime
npm add --save-dev @flue/cli
npx flue init --target node
```

```sh
npm add @rivet-dev/flue-target @rivet-dev/agentos @rivet-dev/agentos-flue
```

- `@rivet-dev/flue-target`: Runs Flue agents and workflows as Rivet Actors.
- `@rivet-dev/agentos`: Provides the isolated VM actor.
- `@rivet-dev/agentos-flue`: Connects Flue's sandbox API to agentOS.

Create `actors.ts`:

```ts title="actors.ts"
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	// Configuration will go here.
});

export const registry = setup({
	use: { vm },
});
```

Update `flue.config.ts`:

```ts title="flue.config.ts"
import { defineConfig } from "@flue/cli/config";
import { rivet } from "@rivet-dev/flue-target";

export default defineConfig({
	target: rivet({ actors: "./actors.ts" }),
});
```

The generated Flue server adds its agent and workflow actors to this registry.

Create `agents/assistant.ts`:

```ts title="agents/assistant.ts"
import { createAgent } from "@flue/runtime";
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { registry } from "../actors";

const sandbox = agentOSSandbox({
	actor: "vm",
	registry,
});

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-5",
	instructions: "Help the user work in the sandboxed repository.",
	sandbox,
}));
```

Set the provider key required by your model, such as `ANTHROPIC_API_KEY`, in `.env`.

```sh
npx flue connect assistant local
```

Flue builds the Rivet target, starts the local Rivet engine, and connects to the `assistant/local` actor.

## Default filesystem

agentOS persists the VM filesystem, including `/workspace`, to Rivet Actor storage by default. Additional mounts can be configured as needed.

## Configuration

### Virtual machine

See the `agentOS()` [configuration reference](/docs/core#configuration-reference) to configure the VM.

### Flue sandbox

`agentOSSandbox()` accepts:

| Option | Required | Description |
| --- | --- | --- |
| `actor` | Yes | Actor registered with `setup()`, such as `vm`. |
| `registry` | Yes | The application registry exported from `actors.ts`. |
| `cwd` | No | Base directory exposed to Flue. Defaults to `/workspace`. |
| `client` | No | An existing client configured for the same registry. |

## Advanced

### agentOS Core sandbox

Use `agentOSCoreSandbox()` when Flue should create agentOS Core VMs directly without Rivet Actor orchestration. The `create` callback owns the complete VM configuration:

```sh
pnpm add @rivet-dev/agentos-core
```

When using agentOS Core instead of regular agentOS, you lose:

- **Durable filesystem and session history.** Core's root filesystem is ephemeral by default, so you must provide your own persistent mount at `/workspace`.
- **Stable per-session actor identity.** Core cannot reconnect to the same VM across Flue process restarts.
- **Automatic sleep and wake.** The VM lives inside Flue's server process. `shutdown()` disposes it.

Use Core only when your application owns equivalent persistence and lifecycle management.
