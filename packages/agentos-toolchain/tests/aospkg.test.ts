import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test } from "vitest";
import { decodeAospkgManifest, packAospkgFromTarBytes } from "../src/aospkg.js";

const dirs: string[] = [];

afterEach(() => {
	for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function sourceTar(): Buffer {
	const dir = mkdtempSync(join(tmpdir(), "agentos-aospkg-runtime-"));
	dirs.push(dir);
	mkdirSync(join(dir, "bin"));
	writeFileSync(join(dir, "bin", "agent"), "#!/usr/bin/env node\n");
	chmodSync(join(dir, "bin", "agent"), 0o755);
	writeFileSync(
		join(dir, "agentos-package.json"),
		JSON.stringify({ name: "runtime-fixture", version: "1.0.0" }),
	);
	const tar = join(dir, "source.tar");
	execFileSync("tar", ["-cf", tar, "-C", dir, "agentos-package.json", "bin"]);
	return readFileSync(tar);
}

describe("package manifest", () => {
	test("packs and decodes v2 software metadata", () => {
		const { bytes } = packAospkgFromTarBytes(sourceTar());
		const manifest = decodeAospkgManifest(bytes);
		expect(bytes.readUInt16LE(16)).toBe(2);
		expect(manifest.name).toBe("runtime-fixture");
		expect(manifest.commands.map((command) => command.command)).toEqual([
			"agent",
		]);
	});
});
