import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const scriptPath = join(dirname(fileURLToPath(import.meta.url)), "check-no-escaping-local-deps.mjs");

function withFixture(fn) {
	const root = mkdtempSync(join(tmpdir(), "escaping-local-deps-"));
	try {
		return fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function write(root, rel, contents) {
	const path = join(root, rel);
	mkdirSync(dirname(path), { recursive: true });
	writeFileSync(path, contents);
}

test("passes in-repo local deps (link/file/path inside the repo)", () => {
	withFixture((root) => {
		write(
			root,
			"registry/package.json",
			JSON.stringify({
				dependencies: { "@secure-exec/core": "link:../packages/core" },
			}),
		);
		write(
			root,
			"tests/fixture/package.json",
			JSON.stringify({ dependencies: { lib: "file:./vendor/lib" } }),
		);
		write(root, "crates/sidecar/Cargo.toml", '[dependencies]\nkernel = { path = "../kernel" }\n');
		execFileSync(process.execPath, [scriptPath, "--root", root], { stdio: "pipe" });
	});
});

test("accepts the sanctioned sibling ../secure-exec escape (npm + cargo)", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/package.json",
			JSON.stringify({
				dependencies: { "@secure-exec/core": "link:../../../secure-exec/packages/core" },
			}),
		);
		write(
			root,
			"crates/sidecar/Cargo.toml",
			'[dependencies]\nsecure-exec-core = { path = "../../../secure-exec/crates/core" }\n',
		);
		execFileSync(process.execPath, [scriptPath, "--root", root], { stdio: "pipe" });
	});
});

test("rejects a package.json local dep that escapes to a non-sibling checkout", () => {
	withFixture((root) => {
		write(
			root,
			"packages/core/package.json",
			JSON.stringify({
				dependencies: { "@secure-exec/core": "link:../../../secure-exec-scratch/packages/core" },
			}),
		);
		const result = spawnSync(process.execPath, [scriptPath, "--root", root], { encoding: "utf8" });
		assert.notEqual(result.status, 0);
		assert.match(result.stderr, /escapes the repo/);
		assert.match(result.stderr, /@secure-exec\/core/);
	});
});

test("rejects a cargo path dep that escapes to a non-sibling checkout", () => {
	withFixture((root) => {
		write(
			root,
			"crates/sidecar/Cargo.toml",
			'[dependencies]\nsecure-exec-core = { path = "../../../other-repo/crates/core" }\n',
		);
		const result = spawnSync(process.execPath, [scriptPath, "--root", root], { encoding: "utf8" });
		assert.notEqual(result.status, 0);
		assert.match(result.stderr, /escapes the repo/);
	});
});
