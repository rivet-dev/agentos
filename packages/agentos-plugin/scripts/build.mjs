#!/usr/bin/env node
// Build wrapper for the agentos-actor-plugin cdylib. Its build.rs also writes
// packages/agentos/src/generated/actor-actions.generated.ts as a side effect.
import { execFileSync } from "node:child_process";

const release = process.argv.includes("--release");

execFileSync(
	"cargo",
	["build", "-p", "agentos-actor-plugin", ...(release ? ["--release"] : [])],
	{ stdio: "inherit" },
);
