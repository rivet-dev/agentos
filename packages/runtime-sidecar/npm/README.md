# @rivet-dev/agentos-runtime-sidecar platform packages

These packages are release artifacts. Each package contains the
`agentos-native-sidecar` binary for one target. They are published by the release
workflow with `npm publish` so the executable bit is preserved.

The meta package `@rivet-dev/agentos-runtime-sidecar` resolves the package for the current
platform at runtime.
