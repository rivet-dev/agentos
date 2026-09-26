# Contributing a Software Package

Software for agentOS VMs is built as an immutable `.aospkg` file and uploaded
to S3-compatible object storage. The `@agentos-software/*` package names are
workspace identities for source/build composition; they are not npm runtime
packages.

## Add a package

1. Copy a package with the same toolchain shape, such as `software/jq`.
2. Add the command source under `software/<name>/native/`.
3. Define commands, aliases, provided files, and registry presentation metadata
   in `agentos-package.json`.
4. Run `pnpm install` and `just software-build <name>`.
5. Run the package tests and `just software-artifacts-dry-run`.

The generated artifact is `software/<name>/dist/package.aospkg`. Do not commit
the staged command binaries, toolchain outputs, or package artifact.

## Test locally

Embedded Core accepts the local artifact path:

```ts
const vm = await AgentOs.create({
	software: [{ packagePath: "/absolute/path/to/package.aospkg" }],
});
```

To test the hosted actor, serve the artifact over HTTPS and pass its URL plus
the digest from the staged `manifest.json` to `software.install`.

## Pull requests

- Use a plain conventional-commit title.
- Include the source, toolchain wiring, manifest, and focused tests.
- Run `cargo check --workspace`, `pnpm build`, `pnpm check-types`, the package
  tests, and the local software artifact staging path.
