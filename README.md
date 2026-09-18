<p align="center">
  <img src=".github/media/banner.png" alt="agentOS" />
</p>

<p align="center">
  Give agents an operating system as a library.<br/>92x faster cold starts, 47x less memory, 254x cheaper than sandboxes.<br/>Built-in ACP agents: Pi, Claude Code, Codex, and OpenCode
</p>

<p align="center">
  <a href="https://rivet.dev/agentos/docs">Documentation</a> | <a href="https://rivet.dev/agentos/docs/quickstart">Quickstart</a> | <a href="https://rivet.dev/agentos/registry">Registry</a> | <a href="https://rivet.dev/discord">Discord</a>
</p>


## Why agentOS

- **Runs inside your process**: No microVMs to boot, no containers to pull, no nested virtualization. Warm VM creation takes single-digit milliseconds and each VM costs tens of megabytes.
- **Embeds in your backend**: Agents call your functions directly via [bindings](https://rivet.dev/agentos/docs/bindings) — ordinary JavaScript calls, not another network service. Credentials stay on the host; agents see only inputs and outputs.
- **Granular security**: [Permissions](https://rivet.dev/agentos/docs/permissions) gate filesystem, network, process, and environment access, with outward-facing capabilities like network egress denied by default. Guest JavaScript runs in V8 isolates and compiled tools run as WebAssembly, all inside one compact runtime.
- **Deploy anywhere**: Just an npm package. Run locally with `npx rivetkit dev`, then deploy to [Rivet Cloud](https://rivet.dev/agentos/docs/deployment) for managed infrastructure or self-host on your own.
- **Open source**: Apache 2.0 licensed.

### agentOS vs Sandbox

agentOS is a lightweight VM that runs inside your process. Sandboxes are full Linux environments. agentOS integrates agents into your backend with [bindings](https://rivet.dev/agentos/docs/bindings) and granular permissions. Sandboxes give you a full OS for browsers, native binaries, and dev servers.

You don't have to choose: agentOS works with sandboxes through [sandbox mounting](https://rivet.dev/agentos/docs/sandbox), spinning up a full sandbox on demand and mounting the sandbox's file system when the workload needs it.

See [agentOS vs Sandbox](https://rivet.dev/agentos/docs/versus-sandbox) for a full comparison.

## Quickstart

```bash
npm install @rivet-dev/agentos @agentos-software/pi
```

Common POSIX utilities (coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip) ship out of the box. [Claude Code](https://rivet.dev/agentos/docs/agents/claude), [Codex](https://rivet.dev/agentos/docs/agents/codex), and [OpenCode](https://rivet.dev/agentos/docs/agents/opencode) install the same way as Pi.

Create the server:

```ts
// server.ts
import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "@agentos-software/pi";

const vm = agentOS({
  software: [pi],
});

export const registry = setup({ use: { vm } });
registry.start();
```

Create the client — any public frontend or another backend:

```ts
// client.ts
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
  endpoint: "http://localhost:6420",
});
const handle = client.vm.getOrCreate("my-agent");

// Subscribe to streaming events. The payload is inferred from the event schema.
const conn = handle.connect();
conn.on("sessionEvent", (event) => {
  console.log(event);
});

// Open a durable session and send a prompt.
await handle.openSession({
  agent: "pi",
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await handle.prompt({
  content: [
    { type: "text", text: "Write a hello world script to /workspace/hello.js" },
  ],
});

// Read the file the agent created
const content = await handle.readFile("/workspace/hello.js");
console.log(new TextDecoder().decode(content));
```

Run both:

```bash
# Terminal 1: start the server
npx tsx server.ts

# Terminal 2: run the client
npx tsx client.ts
```

agentOS can run Node.js and shell scripts inside the VM:

```ts
// Node.js
await handle.writeFile("/hello.mjs", 'import fs from "fs"; fs.writeFileSync("/out.txt", "hi")');
await handle.exec("node /hello.mjs");

// Bash
const result = await handle.exec("cat /out.txt");
console.log(result.stdout); // "hi"
```

`@rivet-dev/agentos` runs each VM as a Rivet Actor with built-in persistence, sleep/wake, multiplayer, preview URLs, and orchestration. To embed VM control in an existing Node.js application without the actor runtime, use [`@rivet-dev/agentos-core`](https://rivet.dev/agentos/docs/quickstart-embedded): `AgentOs.create()` boots a VM and returns a handle you call directly.

See the [Quickstart guide](https://rivet.dev/agentos/docs/quickstart) for the full walkthrough. agentOS is in preview and the API is subject to change — questions and issues welcome on [Discord](https://rivet.dev/discord).

## Benchmarks

All benchmarks compare agentOS against the fastest/cheapest mainstream sandbox providers as of March 30, 2026. Methodology and reproduction steps: [Benchmarks](https://rivet.dev/agentos/docs/benchmarks).

### Cold start

| Percentile | agentOS | Fastest Sandbox (E2B) | Speedup |
|---|---|---|---|
| p50 | 4.8 ms | 440 ms | **92x faster** |
| p95 | 5.6 ms | 950 ms | **170x faster** |
| p99 | 6.1 ms | 3,150 ms | **516x faster** |

<sub>agentOS: measured on Intel i7-12700KF. Sandbox baseline: E2B, the fastest mainstream sandbox provider as of March 30, 2026.</sub>

### Memory per instance

| Workload | agentOS | Cheapest Sandbox (Daytona) | Reduction |
|---|---|---|---|
| Full coding agent (Pi + MCP + filesystem) | ~131 MB | ~1,024 MB | **8x smaller** |
| Simple shell command | ~22 MB | ~1,024 MB | **47x smaller** |

<sub>Sandbox baseline: Daytona minimum instance (1 vCPU + 1 GiB RAM), the cheapest mainstream sandbox provider as of March 30, 2026.</sub>

### Cost per execution-second (self-hosted)

Full coding agent:

| Host tier | agentOS | Cheapest Sandbox (Daytona) | Difference |
|---|---|---|---|
| AWS ARM | $0.00000058/s | $0.000018/s | **32x cheaper** |
| AWS x86 | $0.00000072/s | $0.000018/s | **26x cheaper** |
| Hetzner ARM | $0.000000066/s | $0.000018/s | **281x cheaper** |
| Hetzner x86 | $0.00000011/s | $0.000018/s | **171x cheaper** |

Simple shell command:

| Host tier | agentOS | Cheapest Sandbox (Daytona) | Difference |
|---|---|---|---|
| AWS ARM | $0.000000073/s | $0.000018/s | **254x cheaper** |
| AWS x86 | $0.000000090/s | $0.000018/s | **205x cheaper** |
| Hetzner ARM | $0.000000011/s | $0.000018/s | **1738x cheaper** |
| Hetzner x86 | $0.000000017/s | $0.000018/s | **1061x cheaper** |

<sub>Sandbox baseline: Daytona at $0.0504/vCPU-h + $0.0162/GiB-h (1 vCPU + 1 GiB minimum). Assumes one agent per sandbox and 70% host utilization.</sub>

## Features

### Agents
- **Built-in agents**: Run [Pi](https://rivet.dev/agentos/docs/agents/pi), [Claude Code](https://rivet.dev/agentos/docs/agents/claude) (beta), [Codex](https://rivet.dev/agentos/docs/agents/codex) (beta), and [OpenCode](https://rivet.dev/agentos/docs/agents/opencode) with a unified API, or [bring your own agent](https://rivet.dev/agentos/docs/agents/custom)
- **[Sessions via ACP](https://rivet.dev/agentos/docs/sessions)**: Create, manage, and resume agent sessions over the [Agent Client Protocol](https://agentclientprotocol.com)
- **Universal transcript format**: One transcript format across all agents for debugging, auditing, and comparison
- **[Automatic persistence](https://rivet.dev/agentos/docs/persistence)**: Every conversation is saved and replayable without extra code
- **Framework integrations**: Use agentOS as the sandbox backend for [Vercel Eve](https://rivet.dev/agentos/docs/frameworks/vercel-eve) (beta) and [Flue](https://rivet.dev/agentos/docs/frameworks/flue) (beta)

### Infrastructure
- **[Execution](https://rivet.dev/agentos/docs/processes)**: Run Bash, Node.js, Python, and registry software inside the VM with real processes, subprocesses, shells, and in-VM servers
- **[Mount external storage as a filesystem](https://rivet.dev/agentos/docs/filesystem)**: S3-compatible storage, Google Drive, host directories, or in-memory mounts, attached at boot or dynamically at runtime
- **[Bindings](https://rivet.dev/agentos/docs/bindings)**: Define JavaScript functions that agents call as CLI commands inside the VM
- **[Cron](https://rivet.dev/agentos/docs/cron) and [webhooks](https://rivet.dev/agentos/docs/webhooks)**: Schedule tasks with built-in cron jobs, and trigger agents from external webhooks with your own HTTP server
- **[Browser](https://rivet.dev/agentos/docs/browser)** (beta): Give agents a cloud browser via Browserbase
- **[Sandbox mounting](https://rivet.dev/agentos/docs/sandbox)** (beta): Pair with full sandboxes (E2B, Daytona, etc.) for heavy workloads like browsers or native compilation

### Orchestration
- **[Multiplayer](https://rivet.dev/agentos/docs/multiplayer)**: Multiple clients observe and collaborate with the same agent in real time
- **[Agent-to-agent](https://rivet.dev/agentos/docs/agent-to-agent)**: Agents delegate work to other agents through host-defined bindings
- **[Workflows](https://rivet.dev/agentos/docs/workflows)**: Chain agent tasks into durable workflows with retries, branching, and resumable execution
- **[Authentication](https://rivet.dev/agentos/docs/authentication)**: Integrate with your existing auth model (API keys, OAuth, JWTs)

### Security
- **[Granular permissions](https://rivet.dev/agentos/docs/permissions)**: Control filesystem, network, process, and environment access, with outward-facing capabilities denied by default
- **[Programmatic network control](https://rivet.dev/agentos/docs/networking)**: Allow or deny any outbound connection with per-host rules, and proxy HTTP into VM services with preview URLs
- **[Resource limits](https://rivet.dev/agentos/docs/resource-limits)**: Set precise CPU and memory limits per agent
- **[VM isolation](https://rivet.dev/agentos/docs/security-model)**: Each agent runs in its own VM with no shared state

## Architecture

agentOS runs each agent in a fully virtualized VM. A trusted sidecar process owns every VM's kernel — virtual filesystem, process table, pipes, PTYs, and a virtual network stack — and brokers every guest syscall; nothing the guest does touches the host directly: no real host filesystem, no real host sockets, no real host processes. Guest JavaScript runs on native V8 with its full JIT ([JavaScript runtime](https://rivet.dev/agentos/docs/js-runtime)), and compiled tools run as WebAssembly. Many VMs share one sidecar process, so each additional VM costs a V8 isolate plus kernel state, not an OS process. With `@rivet-dev/agentos`, each VM is a Rivet Actor with durable state.

See the [Architecture docs](https://rivet.dev/agentos/docs/architecture) for details.

## Registry

Extend agentOS with agents, filesystems, browsers, and software from one registry. Browse the full catalog at the [agentOS Registry](https://rivet.dev/agentos/registry).

Common POSIX utilities ship out of the box. The registry adds agents (`@agentos-software/pi`, `@agentos-software/claude-code`, `@agentos-software/codex`, `@agentos-software/opencode`), command packages (`git`, `ripgrep`, `jq`, `sqlite3`, `duckdb`, `curl`, `vim`, and more), meta-packages (`common`, `build-essential`, `everything`), and integrations like the Browserbase cloud browser. Install any of them from npm and pass them via `software: [...]`.

## License

Apache-2.0
