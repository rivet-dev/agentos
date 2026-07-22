# Flue

Use agentOS as the durable sandbox backend for Flue.

Flue owns the agent runtime and session lifecycle, while agentOS maps every sandbox session to an isolated VM actor with a durable `/workspace` filesystem.

[View the complete example →](https://github.com/rivet-dev/agentos/tree/main/examples/flue)

## Quickstart

```sh
mkdir my-agent && cd my-agent
npm add @flue/runtime
npm add --save-dev @flue/cli
npx flue init --target node
```

```sh
npm add @rivet-dev/agentos @rivet-dev/agentos-flue
```

- `@rivet-dev/agentos`: Provides the durable VM actor.
- `@rivet-dev/agentos-flue`: Connects Flue's sandbox API to agentOS.

Create `registry.ts`:

```ts title="registry.ts"
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	// Configuration will go here.
});

export const registry = setup({
	use: { vm },
});
```

Update `agents/assistant.ts`:

```ts title="agents/assistant.ts"
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { createAgent } from "@flue/runtime";
import { registry } from "../registry";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-4-6",
	sandbox: agentOSSandbox({ actor: "vm", registry }),
}));
```

```sh
flue dev
```

## Run the Flue agent on Rivet

Install the Rivet target adapter:

```sh
npm add @rivet-dev/flue
```

Then export your agentOS actors for the target and select Rivet in `flue.config.ts`:

```ts title="registry.ts"
import { agentOS } from "@rivet-dev/agentos";

export const actors = { vm: agentOS() };
```

```ts title="flue.config.ts"
import { defineConfig } from "@flue/cli/config";
import { rivet } from "@rivet-dev/flue";

export default defineConfig({
	target: rivet({ actors: "./registry.ts" }),
});
```

When the agent runs inside the Rivet target, use its installed registry proxy instead of starting a second registry:

```ts title="agents/assistant.ts"
import { agentOSSandbox } from "@rivet-dev/agentos-flue";
import { createAgent } from "@flue/runtime";
import { flueRegistry } from "@rivet-dev/flue/runtime";

export default createAgent(() => ({
	model: "anthropic/claude-sonnet-4-6",
	sandbox: agentOSSandbox({ actor: "vm", registry: flueRegistry }),
}));
```

See the [Rivet Flue guide](https://rivet.dev/docs/integrations/flue) for deployment and connection commands.

## Default filesystem

agentOS persists the VM filesystem, including `/workspace`, to Rivet Actor storage by default. Additional mounts can be configured as needed.

## Configuration

### Virtual machine

See the `agentOS()` [configuration reference](/docs/core#configuration-reference) to configure the VM.

### Flue sandbox

`agentOSSandbox()` accepts:

| Option | Required | Description |
| --- | --- | --- |
| `actor` | Yes | Actor registered with `setup()` or the Rivet target, such as `vm`. |
| `registry` | Yes | The application registry or `flueRegistry` proxy containing that actor. |
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