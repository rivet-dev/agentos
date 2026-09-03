import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
	EXCLUDED,
	assertDiscoverySanity,
	buildMetaPlatformMap,
	discoverPackages,
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

test("discovers the runtime sidecar resolver packages", () => {
	const packages = discoverPackages(repoRoot);
	const names = packages.map((pkg) => pkg.name);

	assert(names.includes("@rivet-dev/agentos-runtime-sidecar-linux-x64-gnu"));
	assert(names.includes("@rivet-dev/agentos-runtime-sidecar"));
	assert(
		names.indexOf("@rivet-dev/agentos-runtime-sidecar-linux-x64-gnu") <
			names.indexOf("@rivet-dev/agentos-runtime-sidecar"),
	);
});

test("builds the runtime sidecar platform map", () => {
	const packages = discoverPackages(repoRoot);
	const names = packages.map((pkg) => pkg.name);
	const metaMap = buildMetaPlatformMap(packages);

	if (names.includes("@rivet-dev/agentos-runtime-sidecar")) {
		assert.deepEqual(metaMap.get("@rivet-dev/agentos-runtime-sidecar"), [
			"@rivet-dev/agentos-runtime-sidecar-darwin-arm64",
			"@rivet-dev/agentos-runtime-sidecar-darwin-x64",
			"@rivet-dev/agentos-runtime-sidecar-linux-arm64-gnu",
			"@rivet-dev/agentos-runtime-sidecar-linux-x64-gnu",
		]);
	}
});

test("sanity check passes for the agent-os workspace", () => {
	const packages = discoverPackages(repoRoot);
	const names = new Set(packages.map((pkg) => pkg.name));

	assert.doesNotThrow(() => assertDiscoverySanity(packages));
	assert(names.has("@rivet-dev/agentos"));
});

test("never publishes registry software packages to npm", () => {
	const names = discoverPackages(repoRoot).map((pkg) => pkg.name);

	assert(!names.some((name) => name.startsWith("@agentos-software/")));
});

test("browser runtime stays explicitly excluded from publication", () => {
	assert(EXCLUDED.has("@rivet-dev/agentos-runtime-browser"));
});
