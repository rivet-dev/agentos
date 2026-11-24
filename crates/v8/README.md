# agentOS V8 binding

Rust bindings from rusty_v8 commit `548adfd54db42c670e0f3fd42211c3af109c0a73`,
paired with agentOS's pinned V8 15.8.62.58 native archives. `UPSTREAM.json`
records the unchanged upstream Rust source digests; license notices are retained
in `licenses/`. This package follows the agentOS release version in lockstep.

Building requires Python 3, a C++23 compiler for the embedding crate, and access
to `releases.rivet.dev`. The build verifies the target archive SHA-256, its source
identity and every member before linking. Linux x64/ARM64 and macOS x64/ARM64 are
supported. Build-time overrides are for trusted toolchains only:
`AGENTOS_RUSTY_V8_SOURCE` can select an already verified bundle directory.

The fixed native configuration uses the bundled C++ runtime. Feature changes
that would disagree with the published native ABI are rejected. Temporal's
native ABI dependencies are pinned to the exact producer versions.

Update the Rust source and native source pins together, preserve upstream
notices, and qualify the complete sidecar and a clean Cargo consumer before
publishing a new binding. Prefer returning to the upstream registry binding
when a compatible version is published and qualified.
