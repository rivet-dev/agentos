import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { promisify } from "node:util";
import { discoverPackages, sidecarPlatforms } from "./packages.js";
import { preparePackedNpm, readPackedNpm } from "./packed-npm.js";

test("packs the discovered release set and verifies identity and digest before publish", async () => {
	const dir = await mkdtemp(join(tmpdir(), "packed-npm-"));
	const version = "1.2.3-rc.1";
	const platforms = sidecarPlatforms();
	const output = join(dir, "out");
	try {
		const packages = discoverPackages(resolve(import.meta.dirname, "../../../.."));
		const fixtures = await Promise.all(packages.map(async (pkg, index) => {
			const path = join(dir, String(index));
			await mkdir(path);
			await writeFile(join(path, "package.json"), JSON.stringify({ name: pkg.name, version }));
			return { ...pkg, dir: path };
		}));
		await preparePackedNpm(fixtures, version, output);
		const packed = await readPackedNpm(output, version, platforms);
		assert.deepEqual(packed.map(({ name }) => name), packages.map(({ name }) => name));

		const fakeBin = join(dir, "bin");
		const calls = join(dir, "calls");
		await mkdir(fakeBin);
		await writeFile(join(fakeBin, "npm"), String.raw`#!/usr/bin/env node
const fs = require("node:fs");
const args = process.argv.slice(2);
if (args[0] !== "publish" || !args.includes("--ignore-scripts") ||
    args[args.indexOf("--tag") + 1] !== "rc" ||
    args[args.indexOf("--registry") + 1] !== "https://registry.npmjs.org") process.exit(1);
fs.appendFileSync(process.env.PUBLISH_TEST_CALLS, args.at(-1) + "\n");
`, { mode: 0o700 });
		await promisify(execFile)(process.execPath, [
			resolve(import.meta.dirname, "../ci/publish-packed-npm.ts"),
			output,
		], {
			env: {
				...process.env,
				PATH: `${fakeBin}:${process.env.PATH}`,
				PUBLISH_TEST_CALLS: calls,
				VERSION: version,
				NPM_TAG: "rc",
				TRIGGER: "release",
				SIDECAR_PLATFORMS: platforms.join(" "),
			},
		});
		const published = (await readFile(calls, "utf8")).trim().split("\n");
		assert.deepEqual(new Set(published.slice(0, platforms.length)), new Set(
			packed.slice(0, platforms.length).map(({ tarball }) => tarball),
		));
		assert.deepEqual(new Set(published), new Set(packed.map(({ tarball }) => tarball)));

		const manifestPath = join(output, "manifest.json");
		const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
		const original = structuredClone(manifest);
		manifest.entries[0].name = "@rivet-dev/agentos-sidecar-fixture";
		await writeFile(manifestPath, JSON.stringify(manifest));
		await assert.rejects(readPackedNpm(output, version, platforms), /unexpected packed npm package entry/);

		await writeFile(manifestPath, JSON.stringify(original));
		await writeFile(packed[0].tarball!, "modified");
		await assert.rejects(readPackedNpm(output, version, platforms), {
			message: `Invalid packed npm digest for ${packed[0].name}`,
		});
	} finally {
		await rm(dir, { recursive: true, force: true });
	}
});
