import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { bumpCargoVersions, bumpPackageJsons } from "./version.js";

async function writeJson(root: string, rel: string, value: unknown) {
	const path = join(root, rel);
	await mkdir(join(path, ".."), { recursive: true });
	await writeFile(path, `${JSON.stringify(value, null, "\t")}\n`);
}

test("bumpCargoVersions bumps [workspace.package] but NOT secure-exec dep requirements", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeFile(
			join(repoRoot, "Cargo.toml"),
			`[workspace.package]
version = "0.2.0"

[workspace.dependencies]
agentos-protocol = { path = "crates/agentos-protocol", version = "0.2.0-rc.3" }
secure-exec-sidecar = { version = "0.3.1-rc.2" }
secure-exec-client = { version = "0.3.1-rc.2" }
serde = "1"
`,
		);

		await bumpCargoVersions(repoRoot, "0.3.0");

		const cargoToml = await readFile(join(repoRoot, "Cargo.toml"), "utf8");
		// a6 workspace version bumped...
		assert.match(cargoToml, /\[workspace\.package\]\nversion = "0\.3\.0"/);
		// ...a6-owned crate dep (path = "crates/...") bumped...
		assert.match(
			cargoToml,
			/agentos-protocol = \{ path = "crates\/agentos-protocol", version = "0\.3\.0" \}/,
		);
		// ...but secure-exec crate dep requirements stay at their registry version.
		assert.match(
			cargoToml,
			/secure-exec-sidecar = \{ version = "0\.3\.1-rc\.2" \}/,
		);
		assert.match(
			cargoToml,
			/secure-exec-client = \{ version = "0\.3\.1-rc\.2" \}/,
		);
		assert.match(cargoToml, /serde = "1"/);
	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});

test("bumpPackageJsons injects agent-os sidecar platform optional dependency", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeJson(repoRoot, "package.json", {
			name: "agentos-workspace",
			private: true,
			packageManager: "pnpm@10.13.1",
		});
		await writeFile(
			join(repoRoot, "pnpm-workspace.yaml"),
			[
				"packages:",
				"  - packages/*",
				"  - packages/sidecar-binary/npm/*",
				"",
			].join("\n"),
		);
		for (const [rel, name] of [
			["packages/core", "@rivet-dev/agentos-core"],
			["packages/sidecar-binary", "@rivet-dev/agentos-sidecar"],
			[
				"packages/sidecar-binary/npm/linux-x64-gnu",
				"@rivet-dev/agentos-sidecar-linux-x64-gnu",
			],
		]) {
			await writeJson(repoRoot, join(rel, "package.json"), {
				name,
				version: "0.0.0",
			});
		}

		await bumpPackageJsons(repoRoot, "0.3.0");

		const sidecarManifest = JSON.parse(
			await readFile(
				join(repoRoot, "packages/sidecar-binary/package.json"),
				"utf8",
			),
		);
		assert.deepEqual(sidecarManifest.optionalDependencies, {
			"@rivet-dev/agentos-sidecar-linux-x64-gnu": "0.3.0",
		});
	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});
