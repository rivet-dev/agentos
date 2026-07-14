import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import test from "node:test";

const repoRoot = resolve(import.meta.dirname, "..");
const generator = join(repoRoot, "scripts/generate-secure-exec-mirror.mjs");

function generate(root) {
	execFileSync(process.execPath, [generator, "--mirror-root", root], {
		cwd: repoRoot,
		stdio: "pipe",
	});
}

function snapshot(root) {
	const files = new Map();
	const visit = (directory) => {
		for (const entry of readdirSync(directory).sort()) {
			const path = join(directory, entry);
			if (statSync(path).isDirectory()) visit(path);
			else files.set(relative(root, path), readFileSync(path));
		}
	};
	visit(root);
	return files;
}

test("secure-exec compatibility mirror generation is idempotent", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-secure-exec-mirror-"));
	try {
		generate(root);
		const first = snapshot(root);
		writeFileSync(join(root, "packages/stale-generated-file"), "stale");
		generate(root);
		assert.deepEqual(snapshot(root), first);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("generated browser compatibility shims stay private and excluded", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-secure-exec-browser-"));
	try {
		generate(root);
		const npmBrowser = JSON.parse(
			readFileSync(join(root, "packages/browser/package.json"), "utf8"),
		);
		assert.equal(npmBrowser.private, true);
		const rustBrowser = readFileSync(
			join(root, "crates/sidecar-browser/Cargo.toml"),
			"utf8",
		);
		assert.match(rustBrowser, /^publish = false$/m);
		const workflow = readFileSync(
			join(root, ".github/workflows/ci.yml"),
			"utf8",
		);
		assert.match(workflow, /--exclude secure-exec-sidecar-browser/);
		const lockfile = readFileSync(join(root, "pnpm-lock.yaml"), "utf8");
		assert.match(lockfile, /^  packages\/browser:$/m);
		assert.match(
			lockfile,
			new RegExp(
				npmBrowser.dependencies["@rivet-dev/agentos-runtime-browser"].replace(
					/[.*+?^${}()|[\]\\]/g,
					"\\$&",
				),
			),
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
