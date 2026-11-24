# @rivet-dev/agentos-build-tools

Workspace-only generators for agentos protocol, V8 bridge, and base filesystem assets.

Run `pnpm --dir packages/build-tools check:generated` to compare the checked-in
sidecar protocol and package-format TypeScript bindings with their BARE schemas.
The check renders both bindings in memory and never rewrites them. If stale, run
`build:protocol` or `build:package-format` from this package and review the diff.
V8 bridge and base-filesystem build outputs are not checked-in bindings and are
not part of this check.
