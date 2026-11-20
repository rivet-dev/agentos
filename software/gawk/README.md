# GNU awk

`@agentos-software/gawk` provides the `awk` command using GNU awk 5.4.1.

Build the command with `just toolchain-cmd awk`, then build this package with
`pnpm --filter @agentos-software/gawk build`. The C toolchain downloads the
checksum-pinned upstream source and builds it against the agentOS POSIX sysroot.
A portability patch uses POSIX spawn for immediate shell execution and command
pipes, preserving GNU awk's interpreter, pipe handling, and child reaping.
LLVM 20 emits finalized WASM exception handling for
`setjmp`/`longjmp`; Binaryen is not required.

This build uses GNU awk's standard numeric implementation. Optional MPFR,
dynamic C extensions, persistent-memory allocation, and translated diagnostics
are disabled through upstream configure options.

The packed package includes the GNU GPL license and source provenance under
`share/licenses/gawk/COPYING` and `share/doc/gawk/SOURCE.json`.

For differential validation, build the same verified upstream release natively
and set `AGENTOS_GAWK_NATIVE_BIN` to its `gawk` executable. Run the nightly tests
with `AGENTOS_GAWK_REQUIRE_NATIVE=1` to require that oracle. Set
`AGENTOS_GAWK_TEST_BIN` to test an isolated WASM build before staging it.
