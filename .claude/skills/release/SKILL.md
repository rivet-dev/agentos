---
name: release
description: Cut a release or a release-preview via the scripts/publish flow. Use when the user asks to release, publish, cut a release, bump the version, or preview a branch.
---

# Release

The publish flow lives in `scripts/publish` and is driven by the unified
`.github/workflows/publish.yaml` workflow. There are two modes:

- **Release** — a versioned cut that publishes to npm + crates.io, creates a
  GitHub release with binaries, and tags `v<version>`.
- **Release-preview** — a branch snapshot published to npm only, under a
  branch-named dist-tag, using fast debug builds. No git tag, no crates.io
  release, no GitHub release.

## Release

A release REQUIRES a real secure-exec release version (the committed deps are
file-based; the workflow verifies `<v>` on npm AND crates.io and release-swaps
to it transiently — cut one first with secure-exec's `release-secure-exec`
skill if needed):

```bash
just release --secure-exec-version <v> --version 0.2.0       # exact version
just release --secure-exec-version <v> --version 0.2.0-rc.1  # rc (npm tag `rc`)
just release --secure-exec-version <v> --patch               # semver bump from latest git tag
```

`just release` runs `scripts/publish/src/local/cut-release.ts`, which:
1. Resolves the version and the `latest` flag (auto-detected from git tags).
2. Validates the working tree is clean and prints a plan to confirm.
3. Rewrites `Cargo.toml` + every publishable `package.json` version.
4. Runs a local core build + type-check fail-fast (`--skip-checks` to skip).
5. Commits + pushes the version bump.
6. Triggers `publish.yaml` with the version + `secure_exec_version`, which
   verifies the secure-exec release, release-swaps the file deps to it in the
   CI checkout (never committed), builds release binaries, publishes npm +
   crates.io, uploads release assets, and tags `v<version>`.

Flags: `--latest` / `--no-latest`, `--dry-run` (mutate files only), `-y`.

## Release-preview

```bash
just release-preview <branch>
```

Dispatches `publish.yaml` on the branch with no version. The context resolver
computes `version = 0.0.0-<sanitized-branch>.<sha>` and `npm_tag = <sanitized-branch>`,
builds a debug sidecar, and publishes every package to npm under that tag.
The `secure-exec-version` job auto-cuts (or reuses) a secure-exec preview at
the committed `.github/refs/secure-exec` sha and release-swaps to it — see the
`release-preview` skill for the end-to-end cross-repo loop (needs the
`SECURE_EXEC_DISPATCH_TOKEN` secret).
Install a preview with:

```bash
npm install @rivet-dev/agentos-core@<sanitized-branch>
```

## Notes

- Never publish to npm or crates.io locally; always go through `publish.yaml`.
- Platform binary packages publish with `npm publish` (preserves the `0755`
  executable bit). `workspace:*` deps are rewritten to literal versions by the
  full `bump-versions` pass before publish, so `npm publish` resolves them.
- `SIDECAR_PLATFORMS` (workflow env + `scripts/publish` discovery) is the single
  source of truth for which platform binary packages are built and published.
- If anything fails, stop and report — do not retry automatically.
