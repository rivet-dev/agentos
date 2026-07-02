---
name: release-preview
description: Cut an agent-os release-preview end-to-end — bump the committed .github/refs/secure-exec sha if needed, then dispatch the preview (which auto-cuts the secure-exec preview at that sha). Use when the user asks for a preview, release-preview, or to hand off a secure-exec change via an agent-os preview build.
---

# Release-preview (agent-os preview, auto-cutting the secure-exec preview)

End-to-end loop for validating secure-exec changes through an agent-os preview.
Both repos use their unified `.github/workflows/publish.yaml`
(workflow_dispatch, no `version` input = preview = debug build, npm-only,
dist-tag = sanitized branch name).

The agent-os committed dependency state is FILE-BASED (`link:`/`path` at
`../secure-exec` + the `.github/refs/secure-exec` sha — see agent-os CLAUDE.md
"Boundaries"). You never pin versions on a branch; the agent-os publish
workflow swaps to published versions transiently (`secure-exec-dep.mjs
release-swap`) inside its own checkout.

## Procedure

1. **Push the secure-exec changes** to a secure-exec branch (jj colocated;
   commit with `jj --config snapshot.max-new-file-size=16777216 ...` if large
   assets complain). Merge to main if they are ready — the ref pin works with
   any reachable sha.

2. **Point agent-os at that sha and push**:

   ```bash
   cd <agent-os>
   just secure-exec-bump <secure-exec-sha>   # writes .github/refs/secure-exec
   jj describe -m "chore(deps): bump secure-exec ref" && jj bookmark set <bm> -r @ && jj git push
   ```

   PR CI clones + builds `../secure-exec` at that sha (cached per sha) — no
   publish needed for CI to go green.

3. **Cut the release-preview**:

   ```bash
   just release-preview <agent-os-branch>
   run=$(gh run list -R rivet-dev/agentos --workflow=publish.yaml -L1 --json databaseId --jq '.[0].databaseId')
   gh run watch -R rivet-dev/agentos "$run" --exit-status
   ```

   The workflow's `secure-exec-version` job automatically cuts (or reuses) a
   secure-exec preview at the committed ref sha — branch `agentos-dep-<sha7>`,
   npm version `0.0.0-agentos-dep-<sha7>.<sha7>`, registry packages under the
   same dist-tag — then every job release-swaps to those versions before
   building/publishing. Requires the `SECURE_EXEC_DISPATCH_TOKEN` secret on the
   agent-os repo (workflow rights on secure-exec).

## Releases

`just release --secure-exec-version <v>` — `<v>` MUST be a real secure-exec
release (verified on npm AND crates.io) so agent-os npm packages and crates.io
crates reference the same secure-exec. Cut a secure-exec release first if
needed. Release-preview is for previews ONLY; never cut a release with it.

## Loop-until-green

On any failure: `gh run view <run> --log-failed`, fix the root cause, re-dispatch,
re-watch. If the auto-cut secure-exec preview half fails, its run is in
rivet-dev/secure-exec's actions under branch `agentos-dep-<sha7>`.

## If a swap ever leaks into the working tree

```bash
node scripts/secure-exec-dep.mjs release-revert && pnpm install
```
