import { mkdir, mkdtemp, readFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { writeSeedAsset } from "../src/footprint/assets.ts";

test("seeds new assets and preserves existing files", async () => {
	const root = await mkdtemp(join(tmpdir(), "pipboy-seeds-"));
	try {
		expect(await writeSeedAsset(root, "commands/sh", Buffer.from("original"))).toBe(true);
		expect(await writeSeedAsset(root, "commands/sh", Buffer.from("replacement"))).toBe(false);
		expect(await readFile(join(root, "commands/sh"), "utf8")).toBe("original");
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

test("rejects ambiguous paths and directory links during asset seeding", async () => {
	const root = await mkdtemp(join(tmpdir(), "pipboy-seeds-"));
	try {
		await mkdir(join(root, "cache"));
		await symlink(join(root, "cache"), join(root, "commands"));
		await expect(writeSeedAsset(root, "commands/sh", Buffer.from("content"))).rejects.toThrow("not a real directory");
		await expect(writeSeedAsset(root, "cache/./sh", Buffer.from("content"))).rejects.toThrow("Invalid package archive path");
		await expect(readFile(join(root, "cache/sh"))).rejects.toMatchObject({ code: "ENOENT" });
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});
