# JavaScript Runtime

How agentOS runs guest JavaScript: native V8 acceleration, low memory overhead, and Node.js compatibility.

The JavaScript runtime is powered by the Rivet [Secure Exec](https://secureexec.dev) project, which provides the isolated V8 runtime that agentOS runs guest code in. Every guest VM executes its JavaScript inside this runtime, fully sandboxed from the host.

## JavaScript Acceleration

- **JavaScript is unusually slow as WebAssembly**: unlike most software, JavaScript pays a heavy penalty when compiled to WebAssembly, because so much engineering has gone into JIT-compiling JavaScript directly in V8.
- **Native V8, full JIT**: agentOS therefore runs guest JavaScript on the native V8 engine with its full JIT compiler, not through a WASM translation layer. We call this **JavaScript Acceleration**.
- **Native-speed execution**: guest JavaScript runs at native speed while staying inside the isolation boundary, with normal Node.js semantics.

## Comparison to Node.js efficiency

- **Isolate model, not processes**: agentOS runs each agent inside a V8 isolate rather than spawning a full Node.js process per agent.
- **Low memory overhead**: an isolate carries far less per-agent memory overhead than a full Node.js process, so many agents fit in the footprint that a process-per-agent model would spend on a handful.
- **Benchmarks**: see the Secure Exec [benchmarks](https://secureexec.dev/docs/benchmarks) for cold start, warm execution, and reuse measurements.

## Node.js compatibility

Guest code runs as Node.js (reporting `process.version` as `v22.0.0`), but it never touches the host runtime. Every `node:` builtin resolves to a kernel-backed bridge or an in-isolate polyfill, never the real host module. For the full builtin matrix (`fs`, `net`, `http`, `crypto`, undici-backed `fetch`, and more), see the Secure Exec [Node.js Compatibility](https://secureexec.dev/docs/nodejs-compatibility) reference.

For the implementation-level event loop—how process-wide Tokio tasks notify a
thread-affine V8 isolate, how readiness is coalesced, and how `Duplex`
backpressure reaches a sidecar-owned socket—see
[JavaScript Executor & Socket Reactor](/docs/architecture/javascript-executor).

### npm packages

By default the VM has no npm packages installed. Mount a host `node_modules` directory to give guest code access to real packages: the `nodeModulesMount` helper projects it read-only at `/root/node_modules`, and the in-kernel resolver walks it exactly like Node.js does, with no bundling or patching.

Resolution matches naive Node.js over the mounted tree: the ancestor `node_modules` walk, `package.json` `exports`/`imports` and conditions, and `realpath`/symlink following (so pnpm and yarn layouts resolve too). Both ESM `import` and CommonJS `require` work. See the Secure Exec [module loading](https://secureexec.dev/docs/features/module-loading) guide for the full model.