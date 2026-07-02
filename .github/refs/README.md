# Sibling checkout refs

Each file here pins the full 40-char git sha of a sibling repository that this
repo's COMMITTED file-based dependencies (`link:`/`path` deps) resolve against.
CI materializes each sibling at its pinned sha; dev machines use their own
checkout at whatever state they're hacking on.

- `secure-exec` — the sha of `rivet-dev/secure-exec` that `../secure-exec`
  link:/path deps build against. Bump with `just secure-exec-bump [sha]`
  (see the `bump-secure-exec` skill); read by
  `scripts/secure-exec-dep.mjs prepare-build` (CI clone), the `verify-file-deps`
  gate, the sibling build cache key in `ci.yml`, and the `secure-exec-version`
  job in `publish.yaml` (preview auto-cut identity `agentos-dep-<sha7>`).

Never edit these by hand — use the bump recipes so the sha is validated and
fully resolved.
