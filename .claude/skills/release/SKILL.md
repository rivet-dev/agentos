---
name: release
description: Cut a release or trigger a preview publish via the scripts/publish flow. Use when the user asks to release, publish, cut a release, bump the version, or preview-publish a branch.
---

# Release

The publish flow lives in `scripts/publish` and is driven by the unified
`.github/workflows/publish.yaml` workflow. There are two modes:

- **Release** — a versioned cut that publishes to npm + crates.io, creates a
  GitHub release with binaries, and tags `v<version>`.
- **Preview publish** — a branch snapshot published to npm only, under a
  branch-named dist-tag, using fast debug builds. No git tag, no crates.io
  release, no GitHub release.

## Release

```bash
just release --version 0.2.0          # exact version
just release --version 0.2.0-rc.1     # rc (npm tag `rc`, prerelease)
just release --patch                  # semver bump from latest git tag
```

`just release` runs `scripts/publish/src/local/cut-release.ts`, which:
1. Resolves the version and the `latest` flag (auto-detected from git tags).
2. Validates the working tree is clean and prints a plan to confirm.
3. Rewrites `Cargo.toml` + every publishable `package.json` version.
4. Runs a local core build + type-check fail-fast (`--skip-checks` to skip).
5. Commits + pushes the version bump.
6. Triggers `publish.yaml` with the version, which builds release binaries,
   publishes npm + crates.io, uploads release assets, and tags `v<version>`.

Flags: `--latest` / `--no-latest`, `--dry-run` (mutate files only), `-y`.

## Preview publish

```bash
just preview-publish <branch>
```

Dispatches `publish.yaml` on the branch with no version. The context resolver
computes `version = 0.0.0-<sanitized-branch>.<sha>` and `npm_tag = <sanitized-branch>`,
builds a debug sidecar, and publishes every package to npm under that tag.
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
