# agentOS Software Catalog

This directory contains the software packages built into immutable `.aospkg`
artifacts for agentOS VMs. Package source and build metadata stay in the pnpm
workspace for local development, but runtime packages are **not published to or
resolved from npm**.

## Build

From the repository root:

```bash
just tools-rebuild
```

For focused work:

```bash
just toolchain-cmd <command>
just software-build
```

Each command package produces `software/<name>/dist/package.aospkg`. Embedded
Core accepts that trusted local path. The hosted `agentOS` actor accepts only an
HTTPS URL and an optional `sha256:<hex>` digest.

## Package layout

```text
software/<name>/
├── package.json
├── agentos-package.json
├── src/index.ts
├── bin/                         # generated, ignored
└── dist/package.aospkg         # generated, ignored
```

`agentos-package.json` is toolchain input. It is encoded into the v2 `.aospkg`
manifest and is not materialized inside the VM. The VFS projects installed
packages under `/opt/agentos/pkgs/<name>/<version>` and links their commands
under `/opt/agentos/bin`.

## Publication

The release workflow stages every built package under a digest-addressed key:

```text
software/packages/<name>/<sha256>.aospkg
software/manifest.json
```

It uploads that directory to S3-compatible object storage. The manifest contains
the package name, exact digest, byte size, and a path relative to the manifest
URL. It is a build inventory, not a semantic-version resolver.

Run the credential-free release staging path locally with:

```bash
just software-artifacts-dry-run
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for adding packages.

## License

Apache-2.0
