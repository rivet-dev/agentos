<p align="center">
  <img src=".github/media/banner.png" alt="agentOS" />
</p>

<p align="center">
  A portable open-source operating system for AI agents.<br/>Near-zero cold starts (~6 ms), up to 32x cheaper than sandboxes.<br/>Built-in ACP agents: Pi, Claude Code, and OpenCode
</p>

<p align="center">
  <a href="https://agentos-sdk.dev/docs">Documentation</a> | <a href="https://agentos-sdk.dev/docs/quickstart">Quickstart</a>
</p>


## Why agentOS

- **Runs inside your process**: No VMs to boot, no containers to pull. Agents start in milliseconds with minimal memory overhead.
- **Embeds in your backend**: Agents call your functions directly via [bindings](https://agentos-sdk.dev/docs/bindings). No network hops, no complex auth between services.
- **Granular security**: Deny-by-default permissions for filesystem, network, and process access. The same isolation technology trusted by browsers worldwide.
- **Deploy anywhere**: Just an npm package. Works on your laptop, Rivet Cloud, Railway, Vercel, Kubernetes, or any container platform.
- **Open source**: Apache 2.0 licensed. Self-host or use [Rivet Cloud](https://agentos-sdk.dev/docs/deployment) for managed infrastructure.

### agentOS vs Sandbox

agentOS is a lightweight VM that runs inside your process. Sandboxes are full Linux environments. agentOS integrates agents into your backend with [bindings](https://agentos-sdk.dev/docs/bindings) and granular permissions. Sandboxes give you a full OS for browsers, native binaries, and dev servers.

You don't have to choose: agentOS works with sandboxes through the [sandbox extension](https://agentos-sdk.dev/docs/sandbox), spinning up a full sandbox on demand and mounting the sandbox's file system when the workload needs it.

## Quick start

```bash
npm install @rivet-dev/agentos-core @agentos-software/common @agentos-software/pi
```

```ts
import { AgentOs } from "@rivet-dev/agentos-core";
import common from "@agentos-software/common";
import pi from "@agentos-software/pi";

const vm = await AgentOs.create({ software: [common, pi] });

// Create a session and send a prompt
const { sessionId } = await vm.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

vm.onSessionEvent(sessionId, (event) => {
  console.log(event);
});

await vm.prompt(sessionId, "Write a hello world script to /home/agentos/hello.js");

// Read the file the agent created
const content = await vm.readFile("/home/agentos/hello.js");
console.log(new TextDecoder().decode(content));

vm.closeSession(sessionId);
await vm.dispose();
```

agentOS can run Node.js and shell scripts inside the VM:

```ts
// Node.js
await vm.writeFile("/hello.mjs", 'import fs from "fs"; fs.writeFileSync("/out.txt", "hi"); console.log(fs.readFileSync("/out.txt", "utf8"));');
await vm.exec("node /hello.mjs");

// Bash
await vm.exec("echo 'hi' > /out.txt && cat /out.txt");
```

See the [Quickstart guide](https://agentos-sdk.dev/docs/quickstart) for the full walkthrough.

## Benchmarks

All benchmarks compare agentOS against the fastest/cheapest mainstream sandbox providers as of March 2026.

### Cold start

| Percentile | agentOS | Fastest Sandbox (E2B) | Speedup |
|---|---|---|---|
| p50 | 4.8 ms | 440 ms | **92x faster** |
| p95 | 5.6 ms | 950 ms | **170x faster** |
| p99 | 6.1 ms | 3,150 ms | **516x faster** |

<sub>agentOS: median of 10,000 runs on Intel i7-12700KF. Sandbox: E2B.</sub>

### Memory per instance

| Workload | agentOS | Cheapest Sandbox (Daytona) | Reduction |
|---|---|---|---|
| Full coding agent (Pi + MCP + filesystem) | ~131 MB | ~1,024 MB | **8x smaller** |
| Simple shell command | ~22 MB | ~1,024 MB | **47x smaller** |

<sub>Sandbox baseline: Daytona minimum (1 vCPU + 1 GiB RAM).</sub>

### Cost per execution (self-hosted)

| Hardware | Cost/sec (agent workload) | vs Sandbox | 
|---|---|---|
| AWS ARM | ~$0.0000032/s | **6x cheaper** |
| AWS x86 | ~$0.0000053/s | **3x cheaper** |
| Hetzner ARM | ~$0.0000011/s | **17x cheaper** |
| Hetzner x86 | ~$0.0000013/s | **14x cheaper** |

<sub>Sandbox baseline: Daytona at $0.0504/vCPU-h + $0.0162/GiB-h. Self-hosted assumes 70% utilization.</sub>

## Features

### Agents
- **Multi-agent support**: Run built-in Pi, Claude Code, and OpenCode agents with a unified API, plus install registry command packages such as Codex as VM software
- **[Sessions via ACP](https://agentos-sdk.dev/docs/sessions)**: Create, manage, and resume agent sessions over the [Agent Communication Protocol](https://agentclientprotocol.com)
- **Universal transcript format**: One transcript format across all agents for debugging, auditing, and comparison
- **[Automatic persistence](https://agentos-sdk.dev/docs/persistence)**: Every conversation is saved and replayable without extra code

### Infrastructure
- **[Mount external storage as a filesystem](https://agentos-sdk.dev/docs/filesystem)**: S3-compatible storage, Google Drive, host directories, overlay filesystems, or custom backends
- **[Bindings](https://agentos-sdk.dev/docs/bindings)**: Define JavaScript functions that agents call as CLI commands inside the VM
- **[Cron](https://agentos-sdk.dev/docs/cron), [webhooks](https://agentos-sdk.dev/docs/webhooks), and queues**: Schedule tasks, receive external events, and serialize work with built-in primitives
- **[Sandbox extension](https://agentos-sdk.dev/docs/sandbox)**: Pair with full sandboxes (E2B, Daytona, etc.) for heavy workloads like browsers or native compilation

### Orchestration
- **[Multiplayer](https://agentos-sdk.dev/docs/multiplayer)**: Multiple clients observe and collaborate with the same agent in real time
- **[Agent-to-agent](https://agentos-sdk.dev/docs/agent-to-agent)**: Agents delegate work to other agents through host-defined bindings
- **[Workflows](https://agentos-sdk.dev/docs/workflows)**: Chain agent tasks into durable workflows with retries, branching, and resumable execution
- **[Authentication](https://agentos-sdk.dev/docs/authentication)**: Integrate with your existing auth model (API keys, OAuth, JWTs)

### Security
- **[Deny-by-default permissions](https://agentos-sdk.dev/docs/permissions)**: Granular control over filesystem, network, process, and environment access
- **[Programmatic network control](https://agentos-sdk.dev/docs/networking)**: Allow, deny, or proxy any outbound connection
- **[Resource limits](https://agentos-sdk.dev/docs/resource-limits)**: Set precise CPU and memory limits per agent
- **[VM isolation](https://agentos-sdk.dev/docs/architecture)**: Each agent runs in its own VM with no shared state

## Architecture

agentOS is built on an in-process operating system kernel. The kernel manages a virtual filesystem, process table, pipes, PTYs, and a virtual network stack. Everything runs inside the kernel -- nothing executes on the host.

See the [Architecture docs](https://agentos-sdk.dev/docs/architecture) for details.

## Registry

Browse pre-built agents, tools, filesystems, and software packages at the [agentOS Registry](https://agentos-sdk.dev/registry).

<!-- BEGIN PACKAGE TABLE -->
### WASM Command Packages

| Package | apt Equivalent | Description | Source | Combined Size | Gzipped |
|---------|---------------|-------------|--------|---------------|---------|
| `@agentos-software/codex-cli` | codex | OpenAI Codex command package (codex, codex-exec) | rust | - | - |
| `@agentos-software/coreutils` | coreutils | GNU coreutils: sh, cat, ls, cp, sort, and 80+ commands | rust | - | - |
| `@agentos-software/curl` | curl | curl HTTP client | c | - | - |
| `@agentos-software/diffutils` | diffutils | GNU diffutils (diff) | rust | - | - |
| `@agentos-software/duckdb` | duckdb | DuckDB command-line interface | c | - | - |
| `@agentos-software/fd` | fd-find | fd fast file finder | rust | - | - |
| `@agentos-software/file` | file | file type detection | rust | - | - |
| `@agentos-software/findutils` | findutils | GNU findutils (find, xargs) | rust | - | - |
| `@agentos-software/gawk` | gawk | GNU awk text processing | rust | - | - |
| `@agentos-software/git` | git | git version control | rust | - | - |
| `@agentos-software/grep` | grep | GNU grep pattern matching (grep, egrep, fgrep) | rust | - | - |
| `@agentos-software/gzip` | gzip | GNU gzip compression (gzip, gunzip, zcat) | rust | - | - |
| `@agentos-software/http-get` | http-get | Minimal HTTP GET fetch helper | c | - | - |
| `@agentos-software/jq` | jq | jq JSON processor | rust | - | - |
| `@agentos-software/ripgrep` | ripgrep | ripgrep fast recursive search | rust | - | - |
| `@agentos-software/sed` | sed | GNU sed stream editor | rust | - | - |
| `@agentos-software/sqlite3` | sqlite3 | SQLite3 command-line interface | c | - | - |
| `@agentos-software/tar` | tar | GNU tar archiver | rust | - | - |
| `@agentos-software/tree` | tree | tree directory listing | rust | - | - |
| `@agentos-software/unzip` | unzip | unzip archive extraction | c | - | - |
| `@agentos-software/yq` | yq | yq YAML/JSON processor | rust | - | - |
| `@agentos-software/zip` | zip | zip archive creation | c | - | - |

### Meta-Packages

| Package | Description | Includes |
|---------|-------------|----------|
| `@agentos-software/build-essential` | Build-essential WASM command set (standard + make + git + curl) | standard, make, git, curl |
| `@agentos-software/common` | Common WASM command set (coreutils + sed + grep + gawk + findutils + diffutils + tar + gzip) | coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip |
| `@agentos-software/everything` | All available WASM command packages in a single bundle | coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip, curl, zip, unzip, jq, ripgrep, fd, tree, file, yq, codex-cli |
<!-- END PACKAGE TABLE -->

## License

Apache-2.0
