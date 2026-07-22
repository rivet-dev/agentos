# `@rivet-dev/agentos-eve`

Use agentOS as the sandbox for Vercel Eve. Choose a Rivet-backed agentOS actor
or a standalone agentOS Core VM without coupling either hosting model to Eve.

Requires Node.js 24 or newer.

## Rivet actor

```sh
pnpm add eve @rivet-dev/agentos @rivet-dev/agentos-eve
```

Register the VM as a normal agentOS actor. Its configuration owns software,
permissions, limits, sandbox mounting, and persistence:

```ts
// registry.ts
import { agentOS, setup } from "@rivet-dev/agentos";

const vm = agentOS({
	// Configure software, permissions, limits, and mounts here.
});

export const registry = setup({
	use: { vm },
});
```

Start that registry from the same process as Eve:

```ts
// agent/instrumentation.ts
import { defineInstrumentation } from "eve/instrumentation";
import { registry } from "../registry";

registry.start();

export default defineInstrumentation({});
```

### Optional: Rivet World

Install `@rivet-dev/vercel-world` to run Eve workflows on Rivet World. Add its
actors to the same registry, then use the app-owned World bootstrap described
in the [integration guide](https://agentos-sdk.dev/docs/frameworks/vercel-eve).

```ts
import { vercelWorldActors } from "@rivet-dev/vercel-world/registry";

export const registry = setup({
	use: { ...vercelWorldActors, vm },
});
```

Give Vercel World that combined registry. In `package.json`, map `#world` to
`./world.ts`, then set `experimental.workflow.world` to `#world` in the Eve
agent config:

```json
{
	"imports": {
		"#world": "./world.ts"
	}
}
```

```ts
// world.ts
import { createWorld as createRivetWorld } from "@rivet-dev/vercel-world";
import { registry } from "./registry.ts";

export function createWorld() {
	// World calls registry.startAndWait() before every actor request. Repeated
	// calls share one readiness promise. Eve instrumentation is not a safe
	// bootstrap because Eve may serve requests before instrumentation completes.
	return createRivetWorld({ registry });
}
```

```ts
// agent/agent.ts
import { defineAgent } from "eve";

export default defineAgent({
	model: "openai/gpt-5.4-mini",
	experimental: { workflow: { world: "#world" } },
});
```

The first World operation lazily starts the registry and waits for the Rivet
envoy. Do not move this into Eve instrumentation: Eve can serve requests before
instrumentation completes.

Select the actor by its registry key:

```ts
// agent/sandbox/sandbox.ts
import { agentOSBackend } from "@rivet-dev/agentos-eve";
import { defineSandbox } from "eve/sandbox";

export default defineSandbox({
	backend: agentOSBackend({ actor: "vm" }),
});
```

Relative paths and command working directories resolve from `/workspace`.
Configure its persistence on the actor—for example with actor durable storage
or a mounted filesystem. The adapter never copies or interprets workspace data.

Each Eve session maps to a stable actor key. `shutdown()` stops processes opened
through Eve and disconnects the client, but does not destroy the actor, so the
session can reattach after actor sleep or process restart.

## Advanced: standalone Core

Install Core instead of the actor package when Rivet orchestration is not
needed:

```sh
pnpm add eve @rivet-dev/agentos-core @rivet-dev/agentos-eve
```

The factory creates one VM per Eve session. All VM configuration and filesystem
persistence remain caller-owned:

```ts
import { AgentOs } from "@rivet-dev/agentos-core";
import { agentOSCoreBackend } from "@rivet-dev/agentos-eve";
import { defineSandbox } from "eve/sandbox";

export default defineSandbox({
	backend: agentOSCoreBackend({
		create: ({ sessionKey }) =>
			AgentOs.create({
				mounts: [
					{
						path: "/workspace",
						plugin: {
							id: "host_dir",
							config: {
								hostPath: `/var/lib/eve/${encodeURIComponent(sessionKey)}`,
							},
						},
						readOnly: false,
					},
				],
			}),
	}),
});
```

Standalone Core has no Rivet orchestration or automatic durable storage.
`shutdown()` stops Eve processes and disposes the caller-created VM.

Network permissions belong to `agentOS(...)` or the Core `create()` factory;
Eve's runtime `setNetworkPolicy()` operation is unsupported.
