# @rivet-dev/agentos-plugin-\<platform\>

Platform-specific prebuilt Agent OS actor plugin cdylib
(`libagentos_actor_plugin.{so,dylib,dll}`).

These packages are **not installed directly**. They are declared as
`optionalDependencies` of [`@rivet-dev/agentos`](../../agentos), so npm installs
only the one matching the current host's `os`/`cpu`/`libc` at install time. The
`@rivet-dev/agentos` runtime resolver (`src/plugin-binary.ts`) then `require`s
the matching package and hands the cdylib path to RivetKit's generic
native-plugin ABI.

The binary is built in CI (`.github/workflows/publish.yaml`, `build-plugin`
job: `cargo build -p agentos-actor-plugin`) and copied into the matching
platform directory before publish. The committed directories contain only the
`package.json` describing the platform.

Supported platforms: `linux-x64-gnu`, `linux-arm64-gnu`, `darwin-arm64`, `darwin-x64`.
