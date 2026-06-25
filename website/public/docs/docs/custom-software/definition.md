# Software Definition

The full software-package definition for custom agents, command packages, and WASM commands in an agentOS VM.

**Software** is anything you install into a VM: an agent, a command package, or a set of WASM commands. Every package, including the built-ins, is declared with `defineSoftware()` and passed to the `software` option.

`defineSoftware()` is a typed identity helper. It returns the descriptor unchanged but gives you full type-checking. The descriptor's `type` field discriminates between the three kinds of software.

## Software types

- **`"agent"`**: a coding agent runnable via `createSession(id)`. Key fields: `agent`, `requires`, `packageDir`.
- **`"tool"`**: command-package software exposed inside the VM. Key fields: `bins`, `requires`, `packageDir`.
- **`"wasm-commands"`**: compiled WASM command binaries. Key fields: `commandDir`, `aliases`, `permissions`.

All descriptors share two base fields:

- **`name`** (required). The software package name.
- **`type`** (required). One of `"agent"`, `"tool"`, or `"wasm-commands"`.

### Agent software

Registers a coding agent. See [Custom Agents](/docs/agents/custom) for the agent-focused guide; this is the full field reference.

- **`packageDir`** (required). This package's directory on the host. Resolves `requires` from its own `node_modules/`, mounted into the VM at `/root/node_modules/<pkg>`.
- **`requires`** (required). npm packages that must be available inside the VM (must include the adapter and agent packages).
- **`agent.id`** (required). The id passed to `createSession(id)`. Reuse a built-in id to override it, or pick a new one.
- **`agent.acpAdapter`** (required). npm package of the ACP adapter, spawned inside the VM. Must be in `requires`.
- **`agent.agentPackage`** (required). npm package of the underlying agent SDK/CLI. Must be in `requires`.
- **`agent.staticEnv`** (optional). Static env vars passed when spawning the adapter (merged **under** user `env`).
- **`agent.env`** (optional). `(ctx: SoftwareContext) => Record<string, string>`; env computed at boot, e.g. to resolve a bin path.
- **`agent.launchArgs`** (optional). Extra CLI args prepended when launching the adapter.
- **`agent.snapshot`** (optional, default `false`). Opt in to evaluating this agent's SDK into the per-sidecar V8 heap snapshot so it is loaded **once per sidecar** and reused across every session, instead of re-evaluated on each `createSession`. This can remove most of the per-session SDK load/eval cost, but only works if the SDK is **snapshot-safe** (see below). SDKs that aren't — or where snapshot creation fails — automatically fall back to the normal per-session dynamic-import path, so this flag never affects correctness, only startup latency.

#### SDK snapshotting & snapshot-safety

A V8 heap snapshot freezes the JavaScript heap *after* the SDK's modules have been evaluated, then seeds a fresh isolate from that frozen heap for each new session. Capturing the heap has hard requirements: the SDK's **module-initialization** code (everything that runs at `import`/`require` time, before any function is called) must not do any of the following, or the snapshot cannot be built and the agent falls back to per-session loading:

- Create **native handles**: load a `.node` addon, instantiate WebAssembly, or otherwise produce a V8 *External*/`Foreign` object at module top level. (Defer these behind a function/lazy `import()` that runs per-session.)
- Open a **file descriptor, socket, timer, or worker**, or leave a **pending promise** at the end of evaluation.
- Read **non-deterministic or per-session state** at top level — `process.env`, cwd, model, `Date.now()`, `Math.random()`, a random UUID — and bake it into a module constant. (Read these *inside* functions instead; per-session config is injected after restore.)

Snapshot-friendly SDKs keep module-init to pure, deterministic work and load anything native/lazy on first use. Set `agent.snapshot: false` (the default) for any SDK that doesn't meet these rules; the agent still runs, just without the shared-snapshot speedup.

### Command package software

Exposes one or more CLI commands inside the VM by mapping a command name to the npm package that provides its `bin`. The descriptor value is still `"tool"` because this is the software package type, separate from host [bindings](/docs/bindings).

- **`packageDir`** (required). This package's directory on the host (resolves `requires`).
- **`requires`** (required). npm packages that must be available inside the VM.
- **`bins`** (required). `Record<commandName, packageName>`, mapping the command name as invoked in the VM to the npm package providing it.

### WASM command software

Registers compiled WebAssembly command binaries (coreutils, ripgrep, jq, …). See [Building Binaries](/docs/custom-software/building-wasm) for how to produce the binaries.

- **`commandDir`** (required). Absolute host path to the directory of `.wasm` command binaries.
- **`aliases`** (optional). `Record<aliasName, targetCommandName>`; symlink-style aliases.
- **`permissions`** (optional). Permission-tier assignments: `full`, `readWrite`, `readOnly` (`string[]` or `"*"`), `isolated`.

Published registry packages (e.g. `@agentos-software/coreutils`) already expose a `commandDir`, so you can pass them to `software` directly without wrapping in `defineSoftware()`. Any object with a `commandDir` property is treated as a WASM command package.

## `SoftwareContext`

The `agent.env` callback (and other dynamic config) receives a `SoftwareContext` for resolving VM-side paths:

```ts
interface SoftwareContext {
  // Resolve a package's bin to its VM path, e.g.
  //   ctx.resolveBin("@mariozechner/pi-coding-agent", "pi")
  //   -> "/root/node_modules/@mariozechner/pi-coding-agent/dist/cli.js"
  resolveBin(packageName: string, binName?: string): string;

  // Resolve a package root to its VM path, e.g.
  //   ctx.resolvePackage("pi-acp") -> "/root/node_modules/pi-acp"
  resolvePackage(packageName: string): string;
}
```

```ts
agent: {
  id: "pi-cli",
  acpAdapter: "pi-acp",
  agentPackage: "@mariozechner/pi-coding-agent",
  env: (ctx) => ({
    PI_ACP_PI_COMMAND: ctx.resolveBin("@mariozechner/pi-coding-agent", "pi"),
  }),
}
```

## Meta-packages

A software entry may be an **array** of descriptors, letting one package bundle several (e.g. a "build-essential" set). Pass arrays directly to `software`:

```ts
const vm = agentOS({
  software: [pi, buildEssential /* = [coreutils, make, git, curl] */],
});
```

## Next steps

- [Custom Agents](/docs/agents/custom): the agent-focused guide.
- [Building Binaries](/docs/custom-software/building-wasm): compile WASM commands and use the registry.
- [Request Software](https://github.com/rivet-dev/agentos/issues/new/choose): ask for a package you need.