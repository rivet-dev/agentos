import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, mkdir, readFile, readdir, rename, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { promisify } from "node:util";
import type { NpmPackage } from "./npm.ts";
import type { Package } from "./packages.ts";

const run = promisify(execFile);
const maxPackages = 64;
const maxManifestBytes = 64 * 1024;
const platformPrefix = "@rivet-dev/agentos-sidecar-";

interface PackedPackage {
	name: string;
	file: string;
	sha512: string;
}

async function digest(path: string): Promise<string> {
	const hash = createHash("sha512");
	for await (const chunk of createReadStream(path)) hash.update(chunk);
	return hash.digest("hex");
}

function tarballName(index: number): string {
	return `package-${index}.tgz`;
}

async function readPackedIdentity(path: string) {
	// Read only package metadata. No files from the archive are extracted.
	const { stdout } = await run("tar", ["-xOzf", path, "package/package.json"], {
		maxBuffer: maxManifestBytes,
	});
	return JSON.parse(stdout) as {
		name?: string;
		version?: string;
		private?: boolean;
		publishConfig?: { registry?: string; access?: string };
	};
}

async function checkIdentity(path: string, name: string, version: string) {
	const pkg = await readPackedIdentity(path);
	if (pkg.private || pkg.name !== name || pkg.version !== version) {
		throw new Error(`Packed npm identity mismatch for ${name}@${version}`);
	}
	const config = pkg.publishConfig ?? {};
	if (Object.entries(config).some(([key, value]) =>
		key === "registry" ? value !== "https://registry.npmjs.org" && value !== "https://registry.npmjs.org/"
		: key !== "access" || value !== "public")) {
		throw new Error(`Unexpected publishConfig for packed npm package ${name}`);
	}
}

export async function preparePackedNpm(
	packages: readonly Package[],
	version: string,
	directory: string,
): Promise<void> {
	if (packages.length === 0 || packages.length > maxPackages) {
		throw new Error(`Expected 1 to ${maxPackages} npm packages`);
	}
	const out = resolve(directory);
	await mkdir(out);
	const entries: PackedPackage[] = [];
	for (const [index, pkg] of packages.entries()) {
		const { stdout } = await run("npm", ["pack", "--json", "--pack-destination", out], {
			cwd: pkg.dir,
			maxBuffer: 8 * 1024 * 1024,
		});
		// npm 11 returns an array. npm 12 keys its result by package name.
		const packResult = JSON.parse(stdout);
		const packed = Array.isArray(packResult) ? packResult[0] : packResult?.[pkg.name];
		if (typeof packed?.filename !== "string" || !/^[A-Za-z0-9][A-Za-z0-9_.+-]*\.tgz$/.test(packed.filename)) {
			throw new Error(`Unexpected npm pack filename for ${pkg.name}`);
		}
		const file = tarballName(index);
		const path = join(out, file);
		await rename(join(out, packed.filename), path);
		await checkIdentity(path, pkg.name, version);
		entries.push({ name: pkg.name, file, sha512: await digest(path) });
	}
	await writeFile(join(out, "manifest.json"), JSON.stringify({ version, entries }), { flag: "wx" });
}

export async function readPackedNpm(
	directory: string,
	version: string,
	platforms: readonly string[],
): Promise<NpmPackage[]> {
	const root = resolve(directory);
	const manifestPath = join(root, "manifest.json");
	const stat = await lstat(manifestPath);
	if (!stat.isFile() || stat.size > maxManifestBytes) {
		throw new Error("Invalid packed npm manifest file");
	}
	const manifest = JSON.parse(await readFile(manifestPath, "utf8")) as {
		version?: unknown;
		entries?: unknown;
	};
	if (!manifest || manifest.version !== version || !Array.isArray(manifest.entries) ||
		manifest.entries.length === 0 || manifest.entries.length > maxPackages) {
		throw new Error("Invalid packed npm manifest version or package count");
	}
	const required = new Set([
		...platforms.map((platform) => platformPrefix + platform),
		"@rivet-dev/agentos",
		"@rivet-dev/agentos-node-pty",
		"@rivet-dev/agentos-sandbox",
		"@rivet-dev/agentos-toolchain",
		"@rivet-dev/agentos-sidecar",
		"secure-exec",
	]);
	const allowed = new Set(required);
	const seen = new Set<string>();
	const files = new Set(await readdir(root));
	files.delete("manifest.json");
	const packages: NpmPackage[] = [];
	for (const [index, entry] of manifest.entries.entries()) {
		const { name, file, sha512 } = (entry ?? {}) as Partial<PackedPackage>;
		if (typeof name !== "string" || !allowed.has(name) || seen.has(name) ||
			file !== tarballName(index) || typeof sha512 !== "string" ||
			!/^[a-f0-9]{128}$/.test(sha512) || !files.delete(file)) {
			throw new Error("Invalid or unexpected packed npm package entry");
		}
		if (index < platforms.length && !name.startsWith(platformPrefix)) {
			throw new Error("Sidecar platform packages must be published first");
		}
		const path = join(root, file);
		if (!(await lstat(path)).isFile() || await digest(path) !== sha512) {
			throw new Error(`Invalid packed npm digest for ${name}`);
		}
		await checkIdentity(path, name, version);
		seen.add(name);
		required.delete(name);
		packages.push({ name, dir: root, relDir: file, tarball: path });
	}
	if (files.size || required.size) throw new Error("Unexpected or missing packed npm artifacts");
	return packages;
}
