#!/usr/bin/env node
// Build wrapper for the agentos-actor-plugin cdylib. Its build.rs also writes
// packages/agentos/src/generated/actor-actions.generated.ts as a side effect.
import { execFileSync } from "node:child_process";

const release = process.argv.includes("--release");

if (process.env.AGENTOS_SKIP_NATIVE_META_BUILD === "1") {
	console.log(
		"Skipping agentos-actor-plugin cargo build; publish CI already staged platform plugin artifacts.",
	);
	process.exit(0);
}

execFileSync(
	"cargo",
	["build", "-p", "agentos-actor-plugin", ...(release ? ["--release"] : [])],
	{ stdio: "inherit" },
);
