# agentOS

agentOS is a lightweight Linux-compatible VM runtime for isolated filesystem,
process, terminal, language, and networking workloads.

It ships in two forms:

- a fixed Rust Rivet actor named `agentOS`, accessed through a generated
  TypeScript contract;
- `@rivet-dev/agentos-core`, the embedded TypeScript API for trusted Node.js
  applications.

The actor is a thin lifecycle and transport wrapper around Core. It does not
contain a configurable TypeScript actor implementation.

## Hosted actor

```bash
npm install @rivet-dev/agentos rivetkit
```

```ts
import { createAgentOsClient } from "@rivet-dev/agentos";

const client = createAgentOsClient();
const vm = client.agentOS.getOrCreate(["workspaces", "demo"], {
  createWithInput: {
    config: {
      filesystem: {
        root: { type: "actor-sqlite", namespace: "root" },
      },
    },
  },
});

await vm.filesystem.writeFile({
  path: "/home/agentos/hello.txt",
  content: "hello\n",
});

const result = await vm.process.run({
  command: "cat",
  args: ["/home/agentos/hello.txt"],
  options: { env: {}, captureStdio: true },
});

console.log(result.stdout);
```

Creation config is used only when the actor is first created. Read the complete
normalized config with `config.get`; replace it with `config.set`. Replacement
is whole-object normalization, so omitted properties return to their defaults.

Hosted actions are grouped under:

- `config` and `vm`
- `filesystem`
- `process` and `terminal`
- `contexts`, `javascript`, `typescript`, and `python`
- `network.fetch`, `network.fetchStream`, and `network.preview`
- `software`
- `cron`

The hosted actor accepts only serializable, whitelisted filesystem descriptors.
It never accepts host mounts, host paths, JavaScript VFS objects, callbacks, or
host functions.

## Software

Software is an immutable v2 `.aospkg` artifact. Hosted actors install HTTPS
URLs:

```ts
await vm.software.install({
  source: {
    url: "https://packages.example.com/ripgrep.aospkg",
    digest: "sha256:<64 lowercase hex characters>",
  },
});
```

The process-wide cache is content-addressed by digest. Release tooling uploads
the catalog to S3-compatible object storage and emits a manifest of relative
artifact URLs, digests, and sizes. Runtime software is not resolved from npm.

## Embedded Core

```bash
npm install @rivet-dev/agentos-core
```

```ts
import { AgentOs } from "@rivet-dev/agentos-core";

const vm = await AgentOs.create();
try {
  await vm.filesystem.writeFile("/tmp/hello.txt", "hello\n");
  const result = await vm.process.exec("cat /tmp/hello.txt", {
    output: { capture: "all" },
  });
  console.log(result.stdout);
} finally {
  await vm.dispose();
}
```

Embedded Core additionally supports trusted local `.aospkg` paths, host-backed
mount plugins, external VM providers, and host functions. Those capabilities stay
inside the embedding process and are intentionally absent from the hosted actor.

Standalone WASM commands use V8 by default. Embedded callers can select
`wasmBackend: "wasmtime"` when their sidecar includes the Wasmtime executor;
JavaScript continues to use V8. See the [embedded API](docs/content/docs/embedded.mdx).

## Development

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm check-types
cargo check --workspace
```

Rebuild the complete command catalog and stage the local object-store release
layout with:

```bash
just tools-rebuild
just software-artifacts-dry-run
```

Public documentation is published from the [docs bundle](docs/).
