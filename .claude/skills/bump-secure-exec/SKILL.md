---
name: bump-secure-exec
description: Advance agent-os's committed secure-exec dependency by bumping the .github/refs/secure-exec sha. Use whenever agent-os needs newer secure-exec (e.g. after a secure-exec PR merges).
---

# Bump the secure-exec ref

The committed deps are file-based (`link:`/`path` at `../secure-exec`); the
ONLY committed version state is the sha in `.github/refs/secure-exec`.

1. Bump it (defaults to the sibling `../secure-exec` checkout's current commit;
   normally pass the merged secure-exec main sha):

   ```bash
   just secure-exec-bump [sha]
   ```

2. Commit + push on the current bookmark:

   ```bash
   jj describe -m "chore(deps): bump secure-exec ref" && jj bookmark set <bm> -r @ && jj git push
   ```

3. CI clones + builds `../secure-exec` at that sha (cached per sha). The FIRST
   run per sha is slow — full native wasm build — then cached.

Rules:
- The sha must be reachable in rivet-dev/secure-exec (merged to main, or a
  pushed branch). Prefer main-merged shas — squash-merges orphan branch commits.
- Never commit published-version pins instead; `verify-file-deps` fails the
  branch. If a swap leaked locally: `node scripts/secure-exec-dep.mjs
  release-revert && pnpm install`.
