import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { DEFAULT_SIDECAR_PLATFORMS } from "./packages.js";
import { bumpCargoVersions, bumpPackageJsons } from "./version.js";

async function writeJson(root: string, rel: string, value: unknown) {
	const path = join(root, rel);
	await mkdir(join(path, ".."), { recursive: true });
	await writeFile(path, `${JSON.stringify(value, null, "\t")}\n`);
}

test("bumpCargoVersions bumps [workspace.package] and agentOS path deps", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeFile(
			join(repoRoot, "Cargo.toml"),
			`[workspace.package]
version = "0.2.0"

[workspace.dependencies]
agentos-acp-protocol = { path = "crates/acp-protocol", version = "0.2.0-rc.3" }
agentos-vm-kernel = { path = "crates/vm-kernel", version = "0.2.0-rc.3" }
serde = "1"
`,
		);
		await mkdir(join(repoRoot, "crates", "excluded-core"), { recursive: true });
		await writeFile(
			join(repoRoot, "crates", "excluded-core", "Cargo.toml"),
			`[package]
name = "agentos-excluded-core"
version = "0.2.0"

[dependencies]
agentos-acp-protocol = { path = "../acp-protocol", version = "0.2.0" }
`,
		);

		await bumpCargoVersions(repoRoot, "0.3.0");

		const cargoToml = await readFile(join(repoRoot, "Cargo.toml"), "utf8");
		// a6 workspace version bumped...
		assert.match(cargoToml, /\[workspace\.package\]\nversion = "0\.3\.0"/);
		// ...agentOS-owned crate deps (path = "crates/...") bumped...
		assert.match(
			cargoToml,
			/agentos-acp-protocol = \{ path = "crates\/acp-protocol", version = "0\.3\.0" \}/,
		);
		assert.match(
			cargoToml,
			/agentos-vm-kernel = \{ path = "crates\/vm-kernel", version = "0\.3\.0" \}/,
		);
		assert.match(cargoToml, /serde = "1"/);
		const excludedCargoToml = await readFile(
			join(repoRoot, "crates", "excluded-core", "Cargo.toml"),
			"utf8",
		);
		assert.match(excludedCargoToml, /version = "0\.3\.0"/);
		assert.match(
			excludedCargoToml,
			/agentos-acp-protocol = \{ path = "\.\.\/acp-protocol", version = "0\.3\.0" \}/,
		);
	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});

test("bumpPackageJsons injects sidecar platform optional dependencies", async () => {
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
				"  - packages/sidecar/npm/*",
				"  - secure-exec",
				"",
			].join("\n"),
		);
		for (const [rel, name] of [
			["packages/agentos", "@rivet-dev/agentos"],
			["packages/core", "@rivet-dev/agentos-core"],
			["packages/sidecar", "@rivet-dev/agentos-sidecar"],
			...DEFAULT_SIDECAR_PLATFORMS.map((platform) => [
				`packages/sidecar/npm/${platform}`,
				`@rivet-dev/agentos-sidecar-${platform}`,
			]),
		]) {
			await writeJson(repoRoot, join(rel, "package.json"), {
				name,
				version: "0.0.0",
			});
		}
		await writeJson(repoRoot, "secure-exec/package.json", {
			name: "secure-exec",
			version: "0.0.1",
			dependencies: { "@rivet-dev/agentos-core": "workspace:*" },
		});

		await bumpPackageJsons(repoRoot, "0.3.0", {
			repository: "rivet-dev/agentos",
		});

		const secureExecManifest = JSON.parse(
			await readFile(join(repoRoot, "secure-exec/package.json"), "utf8"),
		);
		assert.equal(secureExecManifest.version, "0.3.0");
		assert.equal(secureExecManifest.dependencies["@rivet-dev/agentos-core"], "0.3.0");
		assert.deepEqual(secureExecManifest.repository, {
			type: "git",
			url: "https://github.com/rivet-dev/agentos.git",
			directory: "secure-exec",
		});

		const sidecarManifest = JSON.parse(
			await readFile(
				join(repoRoot, "packages/sidecar/package.json"),
				"utf8",
			),
		);
		assert.deepEqual(
			sidecarManifest.optionalDependencies,
			Object.fromEntries(
				DEFAULT_SIDECAR_PLATFORMS.map((platform) => [
					`@rivet-dev/agentos-sidecar-${platform}`,
					"0.3.0",
				]).sort(),
			),
		);

	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});

test("bumpPackageJsons rejects unpublished registry software runtime dependencies", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeJson(repoRoot, "package.json", {
			name: "agentos-workspace",
			private: true,
			packageManager: "pnpm@10.13.1",
		});
		await writeFile(
			join(repoRoot, "pnpm-workspace.yaml"),
			["packages:", "  - packages/*", "  - software/*", ""].join("\n"),
		);
		await writeJson(repoRoot, "packages/core/package.json", {
			name: "@rivet-dev/agentos-core",
			version: "0.0.1",
			dependencies: {
				"@agentos-software/tar": "workspace:*",
			},
		});
		await writeJson(repoRoot, "software/tar/package.json", {
			name: "@agentos-software/tar",
			version: "0.0.1",
		});

		await assert.rejects(
			bumpPackageJsons(repoRoot, "0.0.0-preview.abc1234", {
				repository: "rivet-dev/agentos",
			}),
			/published package @rivet-dev\/agentos-core depends on unpublished workspace package @agentos-software\/tar/,
		);
	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});
