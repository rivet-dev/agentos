# Building the agentOS V8 engine

Normal PR, benchmark and sidecar release jobs use reviewed binary inputs. The
**Build pinned V8 archives** workflow builds them for GNU Linux and macOS on x64
and arm64. It has read-only repository permissions. Each output contains the
static library, Rust bindings, public headers, generated GN configuration, ICU
common data, license notices and the Rust crate files from the pinned Git commit.
The lockstep `agentos-v8` crate includes the unchanged upstream Rust sources
and downloads the reviewed native archive for its target. Normal consumers need
no Git override or Chromium checkout. The producer retains the upstream crate
inside each archive for independent native-build qualification. Before upload, a
separate Cargo build links the bundle and compares the native V8 version to
the generated binding and the pinned source version.

Linux x64 builds use the checksum-pinned glibc 2.27 sysroot in
`linux-x64-sysroot.json` for both native compilation and bindgen. The bundle
records this provenance; older runner-header bundles are rejected. The separate
consumer creates and disposes an isolate/context, links against the release
sysroot, passes the ELF glibc-floor check and runs on Ubuntu 18.04. A full
sidecar release link and runtime checks are still required before release.

Pip-Boy's dependency comparison supports older checkouts with Git-pinned Rust
sources. The current checkout uses `agentos-v8`; a registry-only base uses
Cargo's normal registry resolution. Its platform size jobs pass the reviewed bundle to BuildKit as a
read-only named context and verify the digest inside the build; Actions
credentials stay on the runner.

`source.json` identifies the upstream Rust bindings, the Deno engine base, the
ordered upstream patch commits, each agentOS patch digest, the resulting V8
Git tree and the ICU revision. The cppgc patch includes the generated build
configuration before its first feature check. The Node serializer patch lets
the agentOS `node:v8` writer emit wire format 15 for interchange with Node 24
and 26. V8's default writer still emits format 16. The native reader accepts
both formats.
`prepare` checks the tree and the remaining Git submodule revisions. Windows-only
DebugVisualizers are unused for these targets.

## Refreshing the archives

1. Review the pinned source and run **Build pinned V8 archives** at that commit.
   A push that changes the source pin, patches or build workflow on a
   same-repository branch under `v8-artifacts/` starts the four builds. A
   maintainer can apply the `build-pinned-v8`
   label to that PR to build its current head explicitly, including after
   correcting a build script. Check the run's repository, checked-out head SHA, and each
   target's successful linked version check. The job summary gives the Actions
   artifact ID and SHA-256 of the inner `<target>.tar.gz`.
   Draft CI can run before release promotion by adding
   `{"artifactId": <integer>, "sha256": "<tar digest>"}` for each target in
   `archives.json`. Its read-only Actions token downloads that exact ID;
   the installer independently checks the tar. Updating only the digest file
   leaves the four source builds idle.
   Manual dispatch accepts a single `target` to rebuild a rejected archive
   without rebuilding unaffected platforms.
2. Copy each unchanged `<target>.tar.gz` to the release origin under
   `agent-os/v8/<v8Tree>/<archive-sha256>/<target>.tar.gz`. The same origin
   serves the shipped sidecar binaries. Verify the downloaded bytes match the
   SHA-256 from the trusted workflow before adding the four target/digest pairs
   to `archives.json` as plain digest strings in a reviewed change. Actions
   artifacts expire. The release preflight and packaging containers require
   the durable form for all published platforms.
3. Run the normal sidecar builds, glibc-floor and old-container checks and
   the executor tests. An absent digest stops CI before it can select an
   upstream binary with the same crate version. No digest is credited until
   that target has actually been built and inspected.
4. Before a crates.io release, qualify an independent, clean Rust consumer
   using the packaged `agentos-v8` dependency and its default archive resolution.
   Its source digests must match `crates/v8/UPSTREAM.json`; its provider files
   must match this directory. Publish the binding before its dependent executor
   crates. The release rehearsal verifies every crate from its own tarball.

## Running locally

Use Python 3.8 or later. Source builds need the corresponding rusty_v8 native
prerequisites, including libclang 21.1+ for bindgen. On Linux leave
`CLANG_BASE_PATH` unset to use the pinned Chromium compiler. Set `LIBCLANG_PATH`
to the host's libclang directory. On Linux, set `RUSTY_V8_BINDGEN_RESOURCE_DIR`
to the output of that Clang installation's `clang -print-resource-dir`. The
Rust bindings also need the target-specific `v8-gn.h` that GN generates during the build.

```sh
repo=$PWD
target=x86_64-unknown-linux-gnu
python3 scripts/ci/v8/engine.py prepare "$repo/target/rusty-v8-source"
bash scripts/ci/v8/prepare-linux-x64.sh "$repo/target/v8-release-sysroot" \
  "$repo/target/v8-linux.env"
(
  cd "$repo/target/rusty-v8-source"
  . "$repo/target/v8-linux.env"
  export CARGO_TARGET_DIR="$repo/target/v8-build"
  export BINDGEN_EXTRA_CLANG_ARGS="$BINDGEN_EXTRA_CLANG_ARGS -I$CARGO_TARGET_DIR/$target/release/gn_out/gen/v8/include"
  V8_FROM_SOURCE=1 \
    cargo +1.98.1 build --jobs 2 --locked --release --lib --target "$target"
)
. "$repo/target/v8-linux.env"
python3 scripts/ci/v8/engine.py pack "$repo/target/rusty-v8-source" \
  "$repo/target/v8-build" "$target" "$repo/target/$target.tar.gz"
```

Normal Cargo builds resolve the published archive automatically:

```sh
cargo build --locked -p agentos-sidecar
```

To prefetch and explicitly select an approved archive:

```sh
python3 scripts/ci/v8/engine.py install "$target" "$repo/target/v8-bundles" \
  --shell-env "$repo/target/v8-build.env"
. "$repo/target/v8-build.env"
cargo build --locked -p agentos-sidecar
```

A draft checkout using temporary pins can add `--allow-actions` with an
authenticated `gh` CLI that has read access to the artifact. A locally qualified build can use the same install command with
`--bundle <file> --sha256 <trusted-pack-digest>`. The installer verifies the
entire archive before extracting and checks all inputs against its manifest. The
activation file selects the verified library, generated binding and headers;
the packaged Rust sources remain pinned in `agentos-v8`.
