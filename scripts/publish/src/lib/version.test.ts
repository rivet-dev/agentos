import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { DEFAULT_SIDECAR_PLATFORMS } from "./packages.js";
import {
	bumpCargoVersions,
	bumpPackageJsons,
	githubRepositoryUrl,
} from "./version.js";

async function writeJson(root: string, rel: string, value: unknown) {
	const path = join(root, rel);
	await mkdir(join(path, ".."), { recursive: true });
	await writeFile(path, `${JSON.stringify(value, null, "\t")}\n`);
}

test("bumpCargoVersions bumps [workspace.package] and AgentOS path deps", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeFile(
			join(repoRoot, "Cargo.toml"),
			`[workspace.package]
version = "0.2.0"

[workspace.dependencies]
agentos-sidecar-protocol = { path = "crates/sidecar-protocol", version = "0.2.0-rc.3" }
agentos-kernel = { path = "crates/kernel", version = "0.2.0-rc.3" }
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
agentos-sidecar-protocol = { path = "../sidecar-protocol", version = "0.2.0" }
`,
		);

		await bumpCargoVersions(repoRoot, "0.3.0");

		const cargoToml = await readFile(join(repoRoot, "Cargo.toml"), "utf8");
		// a6 workspace version bumped...
		assert.match(cargoToml, /\[workspace\.package\]\nversion = "0\.3\.0"/);
		// ...AgentOS-owned crate deps (path = "crates/...") bumped...
		assert.match(
			cargoToml,
			/agentos-sidecar-protocol = \{ path = "crates\/sidecar-protocol", version = "0\.3\.0" \}/,
		);
		assert.match(
			cargoToml,
			/agentos-kernel = \{ path = "crates\/kernel", version = "0\.3\.0" \}/,
		);
		assert.match(cargoToml, /serde = "1"/);
		const excludedCargoToml = await readFile(
			join(repoRoot, "crates", "excluded-core", "Cargo.toml"),
			"utf8",
		);
		assert.match(excludedCargoToml, /version = "0\.3\.0"/);
		assert.match(
			excludedCargoToml,
			/agentos-sidecar-protocol = \{ path = "\.\.\/sidecar-protocol", version = "0\.3\.0" \}/,
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
				"  - packages/runtime-sidecar/npm/*",
				"",
			].join("\n"),
		);
		for (const [rel, name] of [
			["packages/agentos", "@rivet-dev/agentos"],
			["packages/core", "@rivet-dev/agentos-core"],
			["packages/runtime-sidecar", "@rivet-dev/agentos-runtime-sidecar"],
			...DEFAULT_SIDECAR_PLATFORMS.map((platform) => [
				`packages/runtime-sidecar/npm/${platform}`,
				`@rivet-dev/agentos-runtime-sidecar-${platform}`,
			]),
		]) {
			const manifest: Record<string, unknown> = {
				name,
				version: "0.0.0",
			};
			if (name === "@rivet-dev/agentos") {
				manifest.dependencies = {
					"@rivet-dev/agentos-core": "workspace:*",
				};
			}
			if (name === "@rivet-dev/agentos-core") {
				manifest.devDependencies = {
					"@agentos-software/common": "workspace:*",
				};
			}
			await writeJson(repoRoot, join(rel, "package.json"), manifest);
		}

		await bumpPackageJsons(repoRoot, "0.3.0", {
			repository: "rivet-dev/agentos",
		});

		const runtimeSidecarManifest = JSON.parse(
			await readFile(
				join(repoRoot, "packages/runtime-sidecar/package.json"),
				"utf8",
			),
		);
		assert.deepEqual(runtimeSidecarManifest.repository, {
			type: "git",
			url: "https://github.com/rivet-dev/agentos.git",
			directory: "packages/runtime-sidecar",
		});
		assert.deepEqual(
			runtimeSidecarManifest.optionalDependencies,
			Object.fromEntries(
				DEFAULT_SIDECAR_PLATFORMS.map((platform) => [
					`@rivet-dev/agentos-runtime-sidecar-${platform}`,
					"0.3.0",
				]).sort(),
			),
		);
		const clientManifest = JSON.parse(
			await readFile(join(repoRoot, "packages/agentos/package.json"), "utf8"),
		);
		assert.equal(
			clientManifest.dependencies["@rivet-dev/agentos-core"],
			"0.3.0",
		);
		const coreManifest = JSON.parse(
			await readFile(join(repoRoot, "packages/core/package.json"), "utf8"),
		);
		assert.deepEqual(coreManifest.devDependencies, {});

	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});

test("bumpPackageJsons rejects unpublished workspace runtime dependencies", async () => {
	const repoRoot = await mkdtemp(join(tmpdir(), "agentos-version-test-"));
	try {
		await writeJson(repoRoot, "package.json", {
			name: "agentos-workspace",
			private: true,
			packageManager: "pnpm@10.13.1",
		});
		await writeFile(join(repoRoot, "pnpm-workspace.yaml"), "packages:\n  - packages/*\n");
		await writeJson(repoRoot, "packages/agentos/package.json", {
			name: "@rivet-dev/agentos",
			version: "0.0.1",
			dependencies: { "@agentos-software/tool": "workspace:*" },
		});

		await assert.rejects(
			bumpPackageJsons(repoRoot, "0.3.0", {
				repository: "rivet-dev/agentos",
			}),
			/unpublished workspace package @agentos-software\/tool/,
		);
	} finally {
		await rm(repoRoot, { recursive: true, force: true });
	}
});

test("githubRepositoryUrl validates owner/repo slugs", () => {
	assert.equal(
		githubRepositoryUrl("rivet-dev/agentos"),
		"https://github.com/rivet-dev/agentos.git",
	);
	assert.throws(() => githubRepositoryUrl("rivet-dev"), /expected owner\/repo/);
	assert.throws(
		() => githubRepositoryUrl("https://github.com/rivet-dev/agentos"),
		/expected owner\/repo/,
	);
});
