# Node.js Runtime

Run Node.js in agentOS: native V8 acceleration, the node CLI, installing packages, and Node.js compatibility.

agentOS runs **Node.js** (`process.version` `v22.0.0`), fully isolated from the host. `node`, `npm`, and `npx` are on the `PATH`.

## JavaScript Acceleration

Normally, JavaScript running inside WebAssembly is exceptionally slow. In agentOS, JavaScript runs inside a native V8 isolate (powered by [Secure Exec](https://secureexec.dev)) for native runtime speeds:

- **Native V8 speed, no overhead** — guest JS runs on V8's full JIT, not a WASM translation layer.
- **Lower memory than a Node.js process** — each agent is a V8 isolate, not a full process, so many fit where process-per-agent fits a handful. See [benchmarks](https://secureexec.dev/docs/benchmarks).

## Running Node

```ts
await agent.exec('node -e "console.log(1 + 1)"');   // inline
await agent.exec("node /workspace/main.js a b");    // script + argv
await agent.exec("npx tsx script.ts");              // npx
await agent.exec('echo "console.log(42)" | node');  // stdin
```

`node` works directly (`exec` / `execArgv` / `spawn`), through the guest shell (`sh -c`, pipes), and as a REPL.

## Installing packages

### Ahead of time

Mount a host `node_modules` tree — projected read-only at `/root/node_modules` and resolved exactly like Node.js (ancestor walk, `package.json` `exports`/`imports`, symlinks — so pnpm/yarn layouts work), for both `import` and `require`:

```ts
import { agentOS, setup, nodeModulesMount } from "@rivet-dev/agentos";

const vm = agentOS({
  mounts: [nodeModulesMount("/absolute/path/to/node_modules")],
});
```

### At runtime

Or install in the VM mid-task:

```ts
await agent.exec("npm install chalk");
await agent.exec("node /workspace/app.js"); // app.js: require("chalk")
```

## Node.js compatibility

Guest code runs as Node.js v22, isolated from the host. `node:` builtins — `fs`, `net`, `http`, `crypto`, undici-backed `fetch`, and more — are provided by the runtime, never the host's. See the full [Node.js Compatibility](https://secureexec.dev/docs/nodejs-compatibility) matrix.