#!/usr/bin/env node
// Committed bin shim: pnpm links workspace bins at install time and silently
// skips missing targets, so pointing `bin` at generated dist/cli.js meant a
// fresh checkout never got the `agentos-toolchain` shim (CI: "agentos-toolchain:
// not found"). This file always exists; dist/cli.js is built before dependents
// run via turbo's ^build ordering.
await import("../dist/cli.js");
