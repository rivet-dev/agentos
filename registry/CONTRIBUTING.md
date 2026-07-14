# Contributing a Registry Package

Software and agent packages for agentOS VMs, published under the
`@agentos-software/*` npm scope. This is the quick path to adding one; the
full documentation lives on the website:

- [Software Definition](https://agentos-sdk.dev/docs/custom-software/definition) — package anatomy and manifest fields
- [Building Binaries](https://agentos-sdk.dev/docs/custom-software/building-wasm) — compiling commands to WASM
- [Publishing Packages](https://agentos-sdk.dev/docs/custom-software/publishing) — shipping to npm with the toolchain

## File structure

```
registry/
├── agent/<pkg>/      JavaScript agent adapters (claude, codex, opencode, pi)
├── software/<pkg>/   WASM command packages (jq, ripgrep, git, …)
└── native/           toolchains that compile the command binaries
```

Each package contains:

```
registry/software/<pkg>/
├── package.json           name, per-package semver, build script
├── agentos-package.json   runtime manifest (commands/aliases/provides) +
│                          `registry` block (title/description/priority/image)
│                          that lists the package on agentos-sdk.dev/registry
├── src/index.ts           descriptor export consumed by `software: []`
├── bin/                   staged binaries (gitignored, built)
└── dist/package/          assembled runtime dir shipped in the npm tarball
```

## Building

From the repo root:

```bash
just registry-native            # compile the fast native wasm command gate
just registry-native-cmd <name> # build one command (required for git, duckdb, vim, wget, codex)
pnpm --filter @agentos-software/<pkg> build # stage bin/ + assemble one package
just registry-build             # stage + assemble every software package
just registry-status            # per-package state
just registry-test              # registry integration tests
```

See [Building Binaries](https://agentos-sdk.dev/docs/custom-software/building-wasm)
for toolchain details (Rust vs C builds, the patched WASI sysroot).

## Adding a package

1. Copy an existing package of the same kind (`software/jq` is a minimal
   example) to `registry/software/<pkg>/`.
2. Add the command source under `registry/native/` (Rust: a
   `crates/commands/<name>` crate; C: wire it into `registry/native/c`).
3. Fill in `agentos-package.json`: `commands` (and `aliases`/`provides` if
   needed) plus a `registry` block with `title` and `description` — without
   that block the package is not listed on the website registry page.
4. Register the directory in `pnpm-workspace.yaml` (it is covered by the
   `registry/software/*` glob) and run `pnpm install`.
5. Run `pnpm --filter @agentos-software/<pkg> build` and check
   `just registry-status`.

See [Software Definition](https://agentos-sdk.dev/docs/custom-software/definition)
for every manifest field.

## Testing in an external project

Inside this repo, tests and examples resolve packages via `workspace:*` — no
publishing needed. To try a package in an external project, pack the built
tarball and install it by path:

```bash
cd registry/software/<pkg>
npm pack                      # produces agentos-software-<pkg>-<version>.tgz
cd /path/to/your-project
npm install /path/to/agentos-software-<pkg>-<version>.tgz
```

Then register it in your VM and run a command:

```typescript
import myPkg from "@agentos-software/<pkg>";
const vm = agentOS({ software: [myPkg] });
```

Real publishes go through `agentos-toolchain publish` (dist-tag `dev` by
default) — see [Publishing Packages](https://agentos-sdk.dev/docs/custom-software/publishing).

## Opening a PR

- Branch, commit with a plain conventional-commit title
  (`feat(registry): add <pkg> package`), no agent attribution.
- Include: the package directory, the native build wiring, and the
  `registry` block so the website picks it up.
- Keep the package version at its own semver (packages version
  independently); never touch other packages' versions or the `latest`
  dist-tag.
- Cheap gates before pushing: `cargo check --workspace`, `pnpm build`,
  `pnpm check-types`, and `pnpm --filter @agentos-software/<pkg> build`.
