import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
	assertDiscoverySanity,
	buildMetaPlatformMap,
	discoverPackages,
	SECURE_EXEC_WORKSPACE_PACKAGES,
} from "./packages.js";

const repoRoot = resolve(import.meta.dirname, "../../../..");

function withFixture(fn: (root: string) => void) {
	const root = mkdtempSync(join(tmpdir(), "publish-packages-"));
	try {
		fn(root);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

function writeJson(root: string, rel: string, value: unknown) {
	const path = join(root, rel);
	mkdirSync(join(path, ".."), { recursive: true });
	writeFileSync(path, `${JSON.stringify(value, null, "\t")}\n`);
}

test("discovers Agent OS sidecar resolver packages", () => {
	const packages = discoverPackages(repoRoot);
	const names = packages.map((pkg) => pkg.name);

	const hasAgentOsPackages = names.some((name) =>
		name.startsWith("@rivet-dev/agentos-"),
	);
	if (hasAgentOsPackages) {
		assert(names.includes("@rivet-dev/agentos-sidecar-linux-x64-gnu"));
		assert(names.includes("@rivet-dev/agentos-sidecar"));
		assert(
			names.indexOf("@rivet-dev/agentos-sidecar-linux-x64-gnu") <
				names.indexOf("@rivet-dev/agentos-sidecar"),
		);
	}

	// a6 no longer publishes the secure-exec runtime packages; they are
	// consumed from npm via the catalog instead.
	assert(!names.includes("@secure-exec/core"));
	assert(!names.includes("@secure-exec/sidecar"));
	assert(!names.includes("@secure-exec/registry-types"));
});

test("discovers secure-exec-only staged packages", () => {
	withFixture((root) => {
		writeJson(root, "package.json", {
			name: "secure-exec-workspace",
			private: true,
			packageManager: "pnpm@10.13.1",
		});
		writeFileSync(
			join(root, "pnpm-workspace.yaml"),
			[
				"packages:",
				"  - packages/*",
				"  - registry/file-system/*",
				"  - registry/tool/*",
				"",
			].join("\n"),
		);
		for (const [rel, name] of [
			["packages/browser", "@secure-exec/browser"],
			["registry/file-system/s3", "@secure-exec/s3"],
			["registry/file-system/google-drive", "@secure-exec/google-drive"],
			["registry/tool/sandbox", "@secure-exec/sandbox"],
		]) {
			writeJson(root, join(rel, "package.json"), {
				name,
				version: "0.0.0",
			});
		}

		const packages = discoverPackages(root);
		const names = packages.map((pkg) => pkg.name);

		assert.deepEqual(
			names.filter((name) => name.startsWith("@rivet-dev/agentos-")),
			[],
		);
		assert(names.includes("@secure-exec/browser"));
		assert(names.includes("@secure-exec/s3"));
		assert(names.includes("@secure-exec/google-drive"));
		assert(names.includes("@secure-exec/sandbox"));
		// a6 no longer discovers the secure-exec runtime packages for publish.
		assert(!names.includes("@secure-exec/core"));
		assert(!names.includes("@secure-exec/sidecar"));
		assert(!names.includes("@secure-exec/registry-types"));
		assert.doesNotThrow(() => assertDiscoverySanity(packages));
	});
});

test("allowlists secure-exec browser package for post-split discovery", () => {
	assert(SECURE_EXEC_WORKSPACE_PACKAGES.has("@secure-exec/browser"));
});

test("builds platform map for the agent-os sidecar meta package", () => {
	const packages = discoverPackages(repoRoot);
	const names = packages.map((pkg) => pkg.name);
	const metaMap = buildMetaPlatformMap(packages);

	if (names.includes("@rivet-dev/agentos-sidecar")) {
		assert.deepEqual(metaMap.get("@rivet-dev/agentos-sidecar"), [
			"@rivet-dev/agentos-sidecar-linux-x64-gnu",
		]);
	}
	// a6 no longer publishes the secure-exec sidecar meta package.
	assert.equal(metaMap.has("@secure-exec/sidecar"), false);
});

test("sanity check passes for the agent-os workspace", () => {
	const packages = discoverPackages(repoRoot);

	assert.doesNotThrow(() => assertDiscoverySanity(packages));
});
