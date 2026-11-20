# agentOS embedded Core

`@rivet-dev/agentos` embeds an agentOS VM in a Node.js application. The
embedding process owns VM lifecycle and may supply trusted host functions,
host-backed mounts, and local `.aospkg` paths.

Install Core directly for embedded use:

```sh
pnpm add @rivet-dev/agentos
```

Use `@rivet-dev/agentos` for the generated TypeScript client for the static Rust
actor. The hosted actor does not expose host functions, host paths, or local
software sources.
