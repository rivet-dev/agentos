# Pipboy footprint provenance

`pipboy pack --source-assets` builds workspace package tarballs with the current
checkout's complete WASM command and default software artifacts. It does not
pack candidate native sidecar binaries. For install-size comparisons, Pipboy
intentionally resolves the sidecar meta package and platform binaries from the
registry's published `@latest`; the separate native check measures candidate
binaries when they are built.

This distinction matters during package rearchitecture. In the 2026-10-29 full
source pack, the new `@rivet-dev/agentos` tarball unpacked to 241.1 MiB, but
the Linux install closure used the historical published
`@rivet-dev/agentos-sidecar-linux-x64-gnu@0.2.22` at 594.6 MiB. The local
candidate release binary was 151 MiB and had not been stripped. Pipboy reported
844.0 MB for Core and 881.7 MB for the sandbox package, over their 550 MB and
600 MB budgets. Those budget errors remain visible; they are not a measurement
of the candidate release's native package sizes. A full release-size comparison
requires the candidate native platform artifacts to be supplied or published.
