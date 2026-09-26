# `@rivet-dev/agentos-toolchain`

Build and pack software for agentOS. The toolchain produces immutable `.aospkg`
files consumed by Core from local paths and by the hosted Rust actor from URLs.

```sh
npx @rivet-dev/agentos-toolchain build ./software/example
npx @rivet-dev/agentos-toolchain pack-aospkg \
	./software/example/dist/package.tar \
	./software/example/dist/package.aospkg
```

The package format is owned by agentOS because the VFS projects packages under
`/opt/agentos`, links their commands under `/opt/agentos/bin`, and dispatches
their executable formats.

## Package inputs

Registry packages contain an `agentos-package.json` toolchain manifest and their
runtime files. `build` stages a deterministic `dist/package.tar`; `pack-aospkg`
encodes that tar as the version 2 package container. The JSON manifest is build
input only and is not materialized in the guest.

The separate `pack` command can turn an npm package or local JavaScript package
into a self-contained `.aospkg`. It is an authoring convenience, not the hosted
package registry or a runtime installation protocol.

```sh
npx @rivet-dev/agentos-toolchain pack ./my-cli --out ./my-cli.aospkg
```

Supported `pack` options are:

| Flag | Meaning |
| --- | --- |
| `--out <path>` | Output `.aospkg` path |
| `--prune-native` | Remove unreachable native `.node` addons |
| `--omit-optional` | Omit optional npm dependencies from the bundled closure |

Hosted actors install the resulting immutable artifact with
`software.install({ source: { url, digest } })`. Embedded Core may install the
same artifact from a trusted local path. Packages contain commands and runtime
files only; there is no workload-orchestration metadata.
