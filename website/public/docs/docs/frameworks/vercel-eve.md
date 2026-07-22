# Vercel Eve

Use agentOS as the durable sandbox backend for Vercel Eve.

Eve owns the agent runtime and session lifecycle, while agentOS maps every sandbox session to an isolated VM actor with a durable `/workspace` filesystem.

[View the complete example →](https://github.com/rivet-dev/agentos/tree/main/examples/vercel-eve)

## Quickstart

```sh
npx eve@latest init my-agent
cd my-agent
```

```sh
npm add @rivet-dev/agentos @rivet-dev/agentos-eve @rivet-dev/vercel-world
```

- `@rivet-dev/agentos`: Provides the durable VM actor.
- `@rivet-dev/agentos-eve`: Connects Eve's sandbox API to agentOS.
- `@rivet-dev/vercel-world`: Runs Eve workflows on [Rivet World](https://workflow-sdk.dev/worlds) (optional)

Update `agent/agent.ts`:

```ts title="agent/agent.ts"
import { defineAgent } from "eve";

export default defineAgent({
	build: {
		externalDependencies: [
			"@rivet-dev/agentos",
			"@rivet-dev/agentos-core",
			"@rivet-dev/agentos-eve",
			"@rivet-dev/agentos-runtime-core",
			"@rivet-dev/agentos-sidecar",
			"@rivet-dev/vercel-world", // Optional: required for Rivet World.
			"@rivetkit/engine-cli",
			"@rivetkit/engine-cli-linux-x64-musl",
		],
	},
	experimental: {
		// Optional: run Eve workflows on Rivet World.
		// The package import maps this to the app-owned World in world.ts.
		workflow: { world: "#world" },
	},
});
```

Create `registry.ts`:

```ts title="registry.ts"
import { agentOS, setup } from "@rivet-dev/agentos";
import { vercelWorldActors } from "@rivet-dev/vercel-world/registry"; // Optional: required for Rivet World.

const vm = agentOS({
	// Configuration will go here.
});

export const registry = setup({
	use: {
		...vercelWorldActors, // Optional: required for Rivet World.
		vm,
	},
});
```

When using Rivet World, create `world.ts`:

```ts title="world.ts"
import { createWorld as createRivetWorld } from "@rivet-dev/vercel-world";
import { registry } from "./registry.ts";

export function createWorld() {
	// World calls registry.startAndWait() before every actor request. Repeated
	// calls share one readiness promise. Eve instrumentation is not a safe
	// bootstrap because Eve may serve requests before instrumentation completes.
	return createRivetWorld({ registry });
}
```

In `package.json`, map the private import used by Eve:

```json title="package.json"
{
	"imports": {
		"#world": "./world.ts"
	}
}
```

The first World operation starts this registry and waits for the Rivet envoy to
be ready. Eve may serve requests before instrumentation finishes, so an
`agent/instrumentation.ts` bootstrap would leave a cold-start race.

Rivet World starts the registry through `world.ts`. Without Rivet World, create
`agent/instrumentation.ts` so Eve starts the runtime before using the sandbox:

```ts title="agent/instrumentation.ts"
import { defineInstrumentation } from "eve/instrumentation";
import { registry } from "../registry";

registry.start();

export default defineInstrumentation({});
```

Create `agent/sandbox/sandbox.ts`:

```ts title="agent/sandbox/sandbox.ts"
import { agentOSBackend } from "@rivet-dev/agentos-eve";
import { defineSandbox } from "eve/sandbox";

export default defineSandbox({
	backend: agentOSBackend({ actor: "vm" }),
});
```

Run the agent:

```sh
eve dev
```

## Default Filesystem

agentOS persists the VM filesystem, including `/workspace`, to Rivet Actor storage by default. Additional mounts can be configured as needed.

## Configuration

### Virtual Machine

See the `agentOS()` [configuration reference](/docs/core#configuration-reference) to configure the VM.

### Eve Sandbox Backend

`agentOSBackend()` accepts:

| Option | Required | Description |
| --- | --- | --- |
| `actor` | Yes | Actor registered with `setup()`, such as `vm`. |

## Advanced

### agentOS Core Backend

Use `agentOSCoreBackend()` when Eve should create agentOS Core VMs directly without Rivet Actor orchestration. The `create` callback owns the complete VM configuration:

```sh
pnpm add @rivet-dev/agentos-core
```

When using agentOS Core instead of regular agentOS, you lose:

- **Durable filesystem and session history.** Core's root filesystem is ephemeral by default, so you must provide your own persistent mount at `/workspace`.
- **Stable per-session actor identity.** Core cannot reconnect to the same VM across Eve process restarts.
- **Automatic sleep and wake.** The VM lives inside Eve's short-lived server process instead staying awake for a given grace period. `shutdown()` disposes it.

Use Core only when your application owns equivalent persistence and lifecycle management.