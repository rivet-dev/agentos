#!/usr/bin/env node
// Build wrapper for the agentos-sidecar binary. Cargo owns incremental caching;
// the resolver finds the result under target/{release,debug}.
import { execFileSync } from "node:child_process";

const release = process.argv.includes("--release");

execFileSync(
	"cargo",
	["build", "-p", "agentos-sidecar", ...(release ? ["--release"] : [])],
	{ stdio: "inherit" },
);
