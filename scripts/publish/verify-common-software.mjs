#!/usr/bin/env node

// Check the actual npm tarballs that a release's default software bundle will
// install. A successful local build does not imply that independently released
// @agentos-software packages on npm contain the same command binaries.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
const common = JSON.parse(
	readFileSync(new URL("../../software/common/package.json", import.meta.url), "utf8"),
);
const useLatest = process.argv.includes("--latest");

function tar(args, input) {
	const result = spawnSync("tar", args, {
		input,
		maxBuffer: 256 * 1024 * 1024,
	});
	if (result.error || result.status !== 0) {
		throw new Error(
			`tar ${args.join(" ")} failed: ${result.error?.message ?? result.stderr.toString("utf8")}`,
		);
	}
	return result.stdout;
}

async function get(url) {
	const response = await fetch(url);
	if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
	return Buffer.from(await response.arrayBuffer());
}

async function verify(name, pinnedVersion) {
	const packageName = name.split("/")[1];
	const manifest = JSON.parse(
		readFileSync(`${repoRoot}/software/${packageName}/agentos-package.json`, "utf8"),
	);
	const required = [
		...(manifest.commands ?? []),
		...Object.keys(manifest.aliases ?? {}),
		...(manifest.stubs ?? []),
	];
	if (required.length === 0) throw new Error(`${name}: no declared commands`);

	const version = useLatest ? "latest" : pinnedVersion;
	if (!version || version.startsWith("workspace:")) {
		throw new Error(`${name}: unresolved version ${JSON.stringify(version)}; run after bump-versions`);
	}
	const metadata = JSON.parse(
		(await get(`https://registry.npmjs.org/${encodeURIComponent(name)}/${version}`)).toString("utf8"),
	);
	if (!useLatest && metadata.version !== pinnedVersion) {
		throw new Error(`${name}: npm returned ${metadata.version}, expected ${pinnedVersion}`);
	}
	const archive = await get(metadata.dist.tarball);
	const aospkg = tar(["-xzOf", "-", "package/dist/package.aospkg"], archive);
	if (aospkg.length < 16 || !aospkg.subarray(0, 4).equals(Buffer.from([0x89, 0x41, 0x4f, 0x53]))) {
		throw new Error(`${name}@${metadata.version}: invalid .aospkg header`);
	}
	const tarOffset = 16 + aospkg.readUInt32LE(8) + aospkg.readUInt32LE(12);
	if (tarOffset >= aospkg.length) {
		throw new Error(`${name}@${metadata.version}: invalid .aospkg payload`);
	}
	const entries = new Set(
		tar(["-tf", "-"], aospkg.subarray(tarOffset))
			.toString("utf8")
			.split("\n")
			.map((entry) => entry.replace(/^\.\//, "")),
	);
	const missing = required.filter((command) => !entries.has(`bin/${command}`));
	if (missing.length > 0) {
		throw new Error(`${name}@${metadata.version}: missing bin/${missing.join(", bin/")}`);
	}
	console.log(`${name}@${metadata.version}: ${required.length} declared commands present`);
}

for (const [name, version] of Object.entries(common.dependencies)) {
	if (!name.startsWith("@agentos-software/")) continue;
	try {
		await verify(name, version);
	} catch (error) {
		console.error(error instanceof Error ? error.message : error);
		process.exitCode = 1;
	}
}
