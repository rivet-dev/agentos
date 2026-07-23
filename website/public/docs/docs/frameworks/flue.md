# Flue

Use agentOS as the durable sandbox backend for Flue.

Flue owns the agent runtime and session lifecycle. Rivet maps each agent instance and workflow run to a durable Rivet Actor, while agentOS gives each Flue context an isolated VM with a persistent `/workspace` filesystem.

This integration currently uses [Rivet's Flue fork](https://github.com/rivet-dev/flue).
We're working to merge its generic target-authoring and runtime extension APIs
[upstream](https://github.com/withastro/flue/discussions/516), allowing Flue to
support actor-model runtimes without taking a Rivet dependency.

[View the complete example →](https://github.com/rivet-dev/agentos/tree/main/examples/flue)

## Quickstart

```sh
mkdir my-agent && cd my-agent
npm add "@flue/runtime@npm:@rivet-dev/labs-flue-runtime"
npm add --save-dev "@flue/cli@npm:@rivet-dev/labs-flue-cli"
npx flue init --target node
```

```sh
npm add @rivet-dev/flue @rivet-dev/agentos @rivet-dev/agentos-flue rivetkit
```

- `@rivet-dev/labs-flue-*`: Rivet-maintained preview builds of Flue's proposed extension APIs.
- `@rivet-dev/flue`: Runs Flue agents and workflows as Rivet Actors.
- `@rivet-dev/agentos`: Provides the isolated VM actor.
- `@rivet-dev/agentos-flue`: Connects Flue's sandbox API to agentOS.

Create `actors.ts`:

Update `flue.config.ts`:

The generated Flue server adds its agent and workflow actors to this registry.
It keeps Flue's native router as the public HTTP service; the Rivet target only
selects and hosts the durable actors behind those routes.

Create `agents/assistant.ts`:

Set the provider key required by your model, such as `ANTHROPIC_API_KEY`, in `.env`.

```sh
npx flue connect assistant local
```

Flue builds the Rivet target, starts the local Rivet engine, and connects to the `assistant/local` actor.

Ask it to use both filesystem and shell operations:

> Write `hello from Flue` to `/workspace/hello.txt`, run `wc -c
> /workspace/hello.txt`, then read the file back.

Reconnect to `assistant/local` and ask it to read the file again. The same Flue
context reconnects to the same agentOS actor and persistent filesystem.

By default, agentOS runs locally with `npx rivetkit dev` — no infrastructure needed. To run in production, deploy to any of these targets:

See [Deployment](/docs/deployment) for managed, self-hosted, and agentOS Core options.

## Runtime model

agentOS does not support Cloudflare Workers yet. It works with Node.js, Bun, or
Deno on platforms such as Railway, Kubernetes, or Vercel.

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
| `params` | No | Connection parameters forwarded to the actor's `onBeforeConnect` hook. |
| `client` | No | An existing client configured for the same registry. |

## Advanced

### agentOS Core sandbox

Use `agentOSCoreSandbox()` when Flue should use caller-owned agentOS Core VMs directly without Rivet Actor orchestration. The `create` callback must return the retained VM for each Flue context:

```sh
pnpm add @rivet-dev/agentos-core
```

When using agentOS Core instead of regular agentOS, you lose:

- **Durable filesystem and session history.** Core's root filesystem is ephemeral by default, so you must provide your own persistent mount at `/workspace`.
- **Stable per-session actor identity.** Core cannot reconnect to the same VM across Flue process restarts.
- **Automatic sleep, wake, and disposal.** The VM lives inside Flue's server process. Flue's sandbox interface does not expose a lifecycle hook, so the `create` callback's owner must retain and dispose every VM it creates.

Use Core only when your application owns equivalent persistence and lifecycle management.
