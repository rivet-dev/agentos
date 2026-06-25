# Building Binaries

Compile WASM command binaries for agentOS from source in the secure-exec registry.

WASM command packages (`type: "wasm-commands"`) ship **compiled `.wasm` binaries** that run inside the VM as guest commands. The binaries are build artifacts and are not checked into git, so to add or change a command you build it from source in the **secure-exec registry**.

You only need this to author new commands. To use existing ones, install the published package (e.g. `@agentos-software/ripgrep`) and pass it to `software`. See [using the registry](#using-the-registry) below.

## Where it lives

Command source and packages live under `registry/` in [secure-exec](https://github.com/rivet-dev/secure-exec/tree/main/registry):

- **`registry/native/crates/`**: the Rust source for the WASM commands.
- **`registry/native/c/`**: the C source for the WASM commands.
- **`registry/software/<name>/`**: the npm package for each command set (`@agentos-software/<name>`). It contains `dist/` (the TypeScript descriptor) and `wasm/` (the compiled binaries it exposes via `commandDir`).

## Build

Build everything from `registry/`:

```bash
make build         # build all WASM binaries + the TypeScript packages
make copy-wasm     # copy binaries into each package's wasm/ directory
make test
```

`copy-wasm` maps each compiled command into `registry/software/<name>/wasm/`, which is the `commandDir` the package exposes. The two toolchains build independently:

### Rust

Most commands are Rust. The source lives in `registry/native/crates/` and compiles for `wasm32-wasip1` with the pinned **nightly** toolchain from `rust-toolchain.toml` (the build vendors and patches `std` for WASI). Build just the Rust commands:

```bash
make build-wasm-rust    # runs: cd native && make wasm
```

### C

C-based commands (e.g. `sqlite3`, `unzip`, `wget`, `zip`) live in `registry/native/c/` and compile with a **wasi-sdk** clang toolchain. Build just the C commands:

```bash
make build-wasm-c       # runs: cd native/c && make programs && make install
```

## Add a new command package

1. Add the command source under `registry/native/crates/` (Rust) or `registry/native/c/` (C).
2. Create `registry/software/<name>/` as an `@agentos-software/<name>` npm package that exports a descriptor with its `commandDir`.
3. Add a copy rule to the `copy-wasm` target mapping the built binary into `registry/software/<name>/wasm/`.
4. If it belongs in a meta-package (e.g. `common` or `build-essential`), add it there.
5. Verify with `make copy-wasm && make build && make test`.

## Let an agent build it

This is a mechanical, well-scoped task, so you can hand it to a coding agent. A prompt like:

```text
Add a WASM command package for `<command>` to the secure-exec registry:
- put the Rust source under registry/native/crates/ (or C under registry/native/c/),
- create registry/software/<command>/ as an @agentos-software/<command> npm
  package that exports a commandDir descriptor,
- add a copy-wasm rule mapping the built binary into its wasm/ directory,
then run `make copy-wasm && make build && make test` and fix any failures.
```

## Using the registry

Install a published package and pass it to `software`. Registry WASM packages expose a `commandDir`, so you pass them directly (no `defineSoftware()` wrapper):

Meta-packages bundle a full set, e.g. `@agentos-software/common` (coreutils, sed, grep, gawk, findutils, diffutils, tar, gzip). Run the commands from the client; see [Processes & Shell](/docs/processes). Browse the full catalog on the [Registry](/registry), and see how packages map onto the `wasm-commands` descriptor in [Software Definition](/docs/custom-software/definition#wasm-command-software).