// Gate: the COMMITTED dependency state must be file-based.
//
// Every @secure-exec/* npm dep and secure-exec-* crate must point at the
// sibling ../secure-exec checkout (link:/path), every @agentos-software/*
// package likewise, and .github/refs/secure-exec must pin the sha CI materializes the
// sibling at. Published-version pins are publish-time-only state produced by
// `secure-exec-dep.mjs release-swap` inside an ephemeral CI checkout — they
// must never land on a branch. (The inverse guard, against links to anywhere
// OTHER than ../secure-exec, is check-no-escaping-local-deps.mjs.)
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];

const refPath = join(ROOT, ".github", "refs", "secure-exec");
if (!existsSync(refPath)) {
	failures.push(".github/refs/secure-exec is missing — run `just secure-exec-bump <sha>`");
} else {
	const sha = readFileSync(refPath, "utf8").trim();
	if (!/^[0-9a-f]{40}$/.test(sha)) {
		failures.push(`.github/refs/secure-exec must hold one full 40-char sha, got "${sha}"`);
	}
}

const status = execFileSync(
	process.execPath,
	[join(ROOT, "scripts", "secure-exec-dep.mjs"), "status"],
	{ encoding: "utf8" },
);
const runtime = /runtime\s.*:\s*(\S+)/.exec(status)?.[1];
const registry = /registry\s.*:\s*(\S+)/.exec(status)?.[1];
if (runtime !== "local") {
	failures.push(
		`runtime deps are "${runtime}" — the committed state must be file-based ` +
			"(link:/path at ../secure-exec). Run `node scripts/secure-exec-dep.mjs release-revert`.",
	);
}
if (registry !== "local") {
	failures.push(
		`registry deps are "${registry}" — the committed state must be file-based. ` +
			"Run `node scripts/secure-exec-dep.mjs release-revert`.",
	);
}

if (failures.length > 0) {
	for (const f of failures) process.stderr.write(`verify-file-deps: ${f}\n`);
	process.exit(1);
}
process.stdout.write("verify-file-deps: OK (committed deps are file-based at ../secure-exec)\n");
