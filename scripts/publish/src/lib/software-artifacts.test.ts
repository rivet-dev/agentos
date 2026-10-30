import { execFileSync } from "node:child_process";
import { packAospkgFromTarBytes } from "../../../../packages/agentos-toolchain/src/aospkg.js";
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

function packedFixture(commands: string[] = ["example"]): Buffer {
	const root = mkdtempSync(join(tmpdir(), "agentos-packed-fixture-"));
	try {
		mkdirSync(join(root, "bin"));
		writeFileSync(join(root, "agentos-package.json"), JSON.stringify({ name: "example", version: "0.0.1" }));
		for (const command of commands) writeFileSync(join(root, "bin", command), "command fixture", { mode: 0o755 });
		return packAospkgFromTarBytes(execFileSync("tar", ["-cf", "-", "-C", root, "agentos-package.json", "bin"])).bytes;
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
}

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
		mkdirSync(join(packageDir, "bin"), { recursive: true });
		writeFileSync(join(packageDir, "bin", "example"), "wasm binary fixture");
		const metaDir = join(root, "software", "meta");
		mkdirSync(metaDir, { recursive: true });
		writeFileSync(
			join(metaDir, "agentos-package.json"),
			JSON.stringify({ registry: { title: "Meta" } }),
		);
		const bytes = packedFixture();
		writeFileSync(join(packageDir, "dist", "package.aospkg"), bytes);
		writeFileSync(
			join(root, "software", "default-software.json"),
			JSON.stringify(["example"]),
		);

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
		assert.deepEqual(
			JSON.parse(readFileSync(result.defaultSoftwarePath, "utf8")),
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

test("rejects a nonempty placeholder package with unstaged commands", () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		const packageDir = join(root, "software", "placeholder");
		mkdirSync(join(packageDir, "dist"), { recursive: true });
		writeFileSync(
			join(packageDir, "package.json"),
			JSON.stringify({ name: "@agentos-software/placeholder", version: "0.0.1" }),
		);
		writeFileSync(
			join(packageDir, "agentos-package.json"),
			JSON.stringify({ commands: ["example"], aliases: { alias: "example" }, stubs: ["stub"] }),
		);
		writeFileSync(join(packageDir, "dist", "package.aospkg"), "nonempty placeholder");
		assert.throws(
			() => stageSoftwareArtifacts(root, join(root, "target", "software-artifacts")),
			/missing staged commands.*example.*alias.*stub/,
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

test("fails when a default package is not in the software catalog", () => {
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
		mkdirSync(join(packageDir, "bin"), { recursive: true });
		writeFileSync(join(packageDir, "bin", "example"), "wasm binary fixture");
		writeFileSync(join(packageDir, "dist", "package.aospkg"), packedFixture());
		writeFileSync(
			join(root, "software", "default-software.json"),
			JSON.stringify(["coreutils"]),
		);
		assert.throws(
			() =>
				stageSoftwareArtifacts(
					root,
					join(root, "target", "software-artifacts"),
				),
			/default software coreutils is not in the software catalog/,
		);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

for (const scenario of [
	{ name: "missing packed commands", bytes: () => packedFixture([]), error: /missing packed commands.*example/ },
	{ name: "truncated packed command payload", bytes: () => {
		const bytes = packedFixture();
		return bytes.subarray(0, 16 + bytes.readUInt32LE(8) + bytes.readUInt32LE(12) + 512);
	}, error: /packed command example has an empty or invalid payload/ },
	{ name: "invalid packed header", bytes: () => Buffer.from("nonempty placeholder"), error: /invalid .aospkg header/ },
]) test(`rejects ${scenario.name} even when loose bins are staged`, () => {
	const root = mkdtempSync(join(tmpdir(), "agentos-software-artifacts-"));
	try {
		const packageDir = join(root, "software", "example");
		mkdirSync(join(packageDir, "dist"), { recursive: true });
		mkdirSync(join(packageDir, "bin"));
		writeFileSync(join(packageDir, "package.json"), JSON.stringify({ name: "@agentos-software/example" }));
		writeFileSync(join(packageDir, "agentos-package.json"), JSON.stringify({ commands: ["example"] }));
		writeFileSync(join(packageDir, "bin", "example"), "staged command");
		writeFileSync(join(packageDir, "dist", "package.aospkg"), scenario.bytes());
		writeFileSync(join(root, "software", "default-software.json"), JSON.stringify(["example"]));
		assert.throws(() => stageSoftwareArtifacts(root, join(root, "target", "software-artifacts")), scenario.error);
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});
