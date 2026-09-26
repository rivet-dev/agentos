# agentOS bindgen prototype

This private workspace package generates the TypeScript contract for the hosted
Rust `agentOS` actor. Rust action registration, event declarations, and wire DTOs
are the source of truth; the generated client continues to use RivetKit's
vanilla nested actor proxy.

From the repository root:

```sh
pnpm --dir packages/agentos-bindgen generate
pnpm --dir packages/agentos-bindgen check
pnpm --dir packages/agentos-bindgen test
```

`generate` updates `schema/agentos.json` and
`packages/agentos/src/generated/contract.ts`. Both files are committed so users
of `@rivet-dev/agentos` do not need a Rust toolchain. `check` regenerates both in
memory and fails if either committed output is stale.

This is intentionally product-specific. It does not modify RivetKit or provide
a general-purpose Rust-to-TypeScript actor binding generator.
