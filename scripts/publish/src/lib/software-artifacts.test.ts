import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
	existsSync,
	mkdtempSync,
	mkdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stageSoftwareArtifacts } from "./software-artifacts.js";

test("stages deterministic digest-addressed .aospkg artifacts", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		const packageDir = join(root, "software", "example");
		mkdirSync(join(packageDir, "dist"), { recursive: true });
		writeFileSync(
			join(packageDir, "package.json"),
			JSON.stringify({ name: "@agentos-software/example", version: "0.0.1" }),
		);
		writeFileSync(
			join(packageDir, "agentos-package.json"),
			JSON.stringify({ commands: ["example"] }),
		);
		const metaDir = join(root, "software", "meta");
		mkdirSync(metaDir, { recursive: true });
		writeFileSync(
			join(metaDir, "agentos-package.json"),
			JSON.stringify({ registry: { title: "Meta" } }),
		);
		const bytes = Buffer.from("aospkg fixture");
		writeFileSync(join(packageDir, "dist", "package.aospkg"), bytes);

		const result = stageSoftwareArtifacts(
			root,
			join(root, "target", "software-artifacts"),
		);
		const digest = createHash("sha256").update(bytes).digest("hex");
		assert.deepEqual(result.manifest, {
			schemaVersion: 1,
			artifacts: [
				{
					name: "example",
					digest: `sha256:${digest}`,
					size: bytes.length,
					path: `packages/example/${digest}.aospkg`,
				},
			],
		});
		assert(
			existsSync(
				join(result.outputDir, "packages", "example", `${digest}.aospkg`),
			),
		);
		assert.deepEqual(
			JSON.parse(readFileSync(result.manifestPath, "utf8")),
			result.manifest,
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("fails when a declared package was not built", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		const packageDir = join(root, "software", "missing");
		mkdirSync(packageDir, { recursive: true });
		writeFileSync(
			join(packageDir, "package.json"),
			JSON.stringify({ name: "@agentos-software/missing", version: "0.0.1" }),
		);
		writeFileSync(
			join(packageDir, "agentos-package.json"),
			JSON.stringify({ commands: ["missing"] }),
		);
		assert.throws(
			() =>
				stageSoftwareArtifacts(
					root,
					join(root, "target", "software-artifacts"),
				),
			/build the complete software catalog/,
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("refuses to replace the repository root", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		assert.throws(() => stageSoftwareArtifacts(root, root), /unsafe/);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test("refuses output outside the repository or with an ambiguous name", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		assert.throws(
			() =>
				stageSoftwareArtifacts(
					root,
					join(root, "..", "software-artifacts"),
				),
			/unsafe/,
		);
		assert.throws(
			() => stageSoftwareArtifacts(root, join(root, "packages")),
			/unsafe/,
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
