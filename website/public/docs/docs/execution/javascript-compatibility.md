# Node.js Compatibility

Node.js builtins available to JavaScript running inside AgentOS.

Guest JavaScript never touches the host Node.js runtime. Every `import` or
`require` of a `node:` builtin resolves to a kernel-backed bridge or an
in-isolate implementation, and unknown or denied modules fail explicitly. The
guest reports Node.js `v22.0.0` through `process.version`.

## How builtins are backed

- **Kernel-backed:** calls route through the VM filesystem, socket table,
  process table, DNS resolver, or entropy source.
- **In-isolate:** pure JavaScript implementations run entirely inside V8 and
  require no host access.
- **Denied:** importing the module throws `ERR_ACCESS_DENIED`.

The canonical inventory lives in
`crates/execution/assets/polyfill-registry.json`.

<Note>A guest never falls through to a real host builtin. Anything not bridged
or implemented in the isolate is denied.</Note>

## Kernel-backed builtins

| Module | Backed by |
| --- | --- |
| `fs`, `fs/promises` | VM filesystem, including fds, streams, metadata, symlinks, and polling-based watchers. |
| `child_process` | VM process table. `spawn`, `exec`, `execFile`, and sync variants launch guest processes. |
| `net`, `dgram` | VM TCP, Unix-socket, and UDP tables. |
| `dns`, `dns/promises` | VM DNS resolver. |
| `http`, `https`, `http2`, `tls` | VM socket and TLS paths, including clients, servers, and connection pooling. |
| `os` | VM-scoped platform, architecture, hostname, CPU, memory, and user information. |
| `crypto` | Entropy, hashes, HMAC, ciphers, scrypt, UUIDs, and WebCrypto. |
| `process` | VM environment, working directory, signals, timers, stdio, and umask. |
| `module` | `createRequire`, builtin resolution, and basic `Module` compatibility. |
| `console` | Bounded formatting and guest stdout/stderr. |
| `readline`, `sqlite`, `tty` | Kernel-backed compatibility surfaces. |
| `timers`, `timers/promises` | Timeout, interval, immediate, and promise variants. |
| `stream/web`, `stream/consumers`, `stream/promises` | Web Streams and stream helpers. |

Network builtins obey the VM's [permission policy](/docs/permissions). Network
access is denied until the VM creator grants it.

## In-isolate builtins

Pure JavaScript implementations include `path`, `buffer`, `events`, `stream`,
`util`, `assert`, `url`, `querystring`, `string_decoder`, `zlib`, `punycode`,
`constants`, and `sys`. Default and named ESM imports are supported.

Compatibility shims are also provided for common feature detection, including
`async_hooks`, `diagnostics_channel`, `perf_hooks`, `worker_threads`, `vm`, and
`v8`. The `worker_threads` shim does not create real worker threads.

## Denied builtins

`cluster`, `domain`, `inspector`, `repl`, `trace_events`, and `wasi` are denied.

## Global APIs

Modern web globals are available, including `fetch`, `Headers`, `Request`,
`Response`, `TextEncoder`, `TextDecoder`, `Buffer`, URL APIs, `Blob`, `File`,
`FormData`, abort APIs, `structuredClone`, `performance`, and WebAssembly.
`fetch()` uses the VM socket table and follows the same policy as `http` and
`net`.

## Modules and output

Both ESM and CommonJS use the VM filesystem and normal `node_modules`
resolution. Console and stream output is delivered through the same bounded
process-output path described in [Processes & Shells](/docs/processes).

Return to the [JavaScript guide](/docs/execution/javascript) for TypeScript,
packages, files, processes, networking, bindings, permissions, and limits.