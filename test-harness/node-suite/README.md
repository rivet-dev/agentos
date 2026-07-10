# Node suite harness

This harness runs the pinned Node 24 `test/parallel` and `test/sequential`
sources vendored by `crates/node-stdlib/scripts/vendor-node.mjs` inside AgentOS
VMs. It uses the same runner for `AGENTOS_JS_STDLIB=legacy` and `real`, parses
Node's `// Flags:` directives, records typed skip reasons for unavailable
features, bounds every test with a timeout, and compares results with an exact
checked-in ledger.

The default `sanity` slice is the cheap PR tier. `smoke` selects a stable 200
test slice for nightly CI, and `full` enumerates the entire parallel/sequential
corpus for nightly and pre-release use.

```sh
AGENTOS_JS_STDLIB=legacy pnpm --dir test-harness node-suite -- --slice sanity
AGENTOS_JS_STDLIB=real pnpm --dir test-harness node-suite -- --slice sanity
pnpm --dir test-harness node-suite -- --slice full --flavor real --write results/real.json
```

Ledger updates are explicit: pass `--update-ledger` together with `--ledger`.
Ordinary runs fail on regressions and unexpected passes. Sanity, smoke, and
full tiers have separate ledgers because their fixture/order and timeout
semantics differ; all use the same state schema and pass ratchet.

Cases with a pass/pass full-suite contract that does not reproduce under smoke
fixture ordering are explicit `tier-variance` skips in smoke. They remain active
in the full-suite ledger, so the full nightly still detects regressions in either
flavor.

M0's full pinned denominator is 3,950 JavaScript tests per flavor. Legacy and
real each pass 647, have 2,015 accepted failures, and skip 1,288 with reasons.
The equal totals hide an exact three-for-three set swap recorded in
`m0-summary.json`; later milestones ratchet those IDs through `ledger.json`.
