import { lstat, mkdir, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { Effect, FileSystem, Schema } from "effect";
import { ASSET_SEEDS } from "../config.ts";
import { npmEnv, run } from "../exec.ts";
import { readTarball } from "./tarball.ts";

const PackOutput = Schema.fromJsonString(Schema.Tuple([Schema.Struct({ filename: Schema.String })]));

function archivePathParts(path: string): string[] {
	const parts = path.split("/");
	if (path.includes("\\") || parts.some((part) => !part || part === "." || part === "..")) {
		throw new Error("Invalid package archive path");
	}
	return parts;
}

export async function writeSeedAsset(root: string, target: string, data: Uint8Array): Promise<boolean> {
	const parts = archivePathParts(target);
	let path = resolve(root);
	for (const part of parts.slice(0, -1)) {
		path = join(path, part);
		await mkdir(path).catch((error: NodeJS.ErrnoException) => {
			if (error.code !== "EEXIST") throw error;
		});
		if (!(await lstat(path)).isDirectory()) {
			throw new Error(`Package asset directory is not a real directory: ${path}`);
		}
	}

	try {
		await writeFile(join(path, basename(target)), data, { flag: "wx" });
		return true;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
		return false;
	}
}

/**
 * Copies the assets in `ASSET_SEEDS` from the latest published tarballs into
 * the checkout at `root`, where the build expects them. Files that already
 * exist, such as a local build's output, are left alone.
 */
export const seedAssets = Effect.fn("Footprint.seedAssets")(function* (root: string) {
	const fs = yield* FileSystem.FileSystem;
	const dir = yield* fs.makeTempDirectoryScoped({ prefix: "pipboy-assets-" });
	let seeded = 0;
	for (const seed of ASSET_SEEDS) {
		const output = yield* run("npm", ["pack", "--ignore-scripts", `${seed.package}@latest`, "--pack-destination", dir, "--json"], { env: npmEnv() });
		const [{ filename }] = yield* Schema.decodeUnknownEffect(PackOutput)(output);
		if (archivePathParts(filename).length !== 1) throw new Error("Invalid npm pack filename");
		for (const entry of readTarball(yield* fs.readFile(join(dir, filename)))) {
			if (!entry.path.startsWith(seed.from)) continue;
			const archivePath = entry.path.slice(seed.from.length);
			archivePathParts(archivePath);
			const target = seed.to(archivePath);
			if (target === undefined) continue;
			if (yield* Effect.promise(() => writeSeedAsset(root, target, entry.data))) seeded++;
		}
	}
	return seeded;
});
