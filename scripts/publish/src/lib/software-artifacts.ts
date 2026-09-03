import { createHash } from "node:crypto";
import {
	cpSync,
	existsSync,
	mkdirSync,
	readFileSync,
	readdirSync,
	rmSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";

export interface SoftwareArtifact {
	name: string;
	digest: string;
	size: number;
	/** Relative URL from this manifest to the immutable package object. */
	path: string;
}

export interface SoftwareArtifactManifest {
	schemaVersion: 1;
	artifacts: SoftwareArtifact[];
}

export interface StageSoftwareArtifactsResult {
	manifest: SoftwareArtifactManifest;
	manifestPath: string;
	outputDir: string;
}

function safeOutputDirectory(repoRoot: string, outputDir: string): string {
	const root = resolve(repoRoot);
	const output = resolve(outputDir);
	const rel = relative(root, output);
	if (
		output === root ||
		output === dirname(output) ||
		rel === ".." ||
		rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) ||
		isAbsolute(rel) ||
		basename(output) !== "software-artifacts"
	) {
		throw new Error(`refusing to replace unsafe software artifact output: ${output}`);
	}
	return output;
}

function readPackageIdentity(packageJsonPath: string): string {
	const raw = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
		name?: unknown;
	};
	if (typeof raw.name !== "string" || !raw.name.startsWith("@agentos-software/")) {
		throw new Error(`invalid registry software name in ${packageJsonPath}`);
	}
	const name = raw.name.slice("@agentos-software/".length);
	if (!/^[a-z0-9][a-z0-9-]*$/.test(name)) {
		throw new Error(`unsafe registry software name in ${packageJsonPath}: ${name}`);
	}
	return name;
}

function isRuntimePackage(manifestPath: string): boolean {
	const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as {
		commands?: unknown;
		name?: unknown;
	};
	return (
		(Array.isArray(manifest.commands) && manifest.commands.length > 0) ||
		(typeof manifest.name === "string" && manifest.name.length > 0)
	);
}

/**
 * Assemble the current software catalog as immutable, digest-addressed files.
 * Directories without an `agentos-package.json` are local meta packages and do
 * not produce remote artifacts.
 */
export function stageSoftwareArtifacts(
	repoRootInput: string,
	outputDirInput: string,
): StageSoftwareArtifactsResult {
	const repoRoot = resolve(repoRootInput);
	const outputDir = safeOutputDirectory(repoRoot, outputDirInput);
	const softwareRoot = join(repoRoot, "software");
	const artifacts: SoftwareArtifact[] = [];
	const seen = new Set<string>();

	rmSync(outputDir, { recursive: true, force: true });
	mkdirSync(join(outputDir, "packages"), { recursive: true });

	for (const entry of readdirSync(softwareRoot, { withFileTypes: true }).sort(
		(a, b) => a.name.localeCompare(b.name),
	)) {
		if (!entry.isDirectory()) continue;
		const packageDir = join(softwareRoot, entry.name);
		const packageManifest = join(packageDir, "agentos-package.json");
		if (!existsSync(packageManifest) || !isRuntimePackage(packageManifest)) continue;
		const name = readPackageIdentity(join(packageDir, "package.json"));
		if (seen.has(name)) throw new Error(`duplicate registry software name: ${name}`);
		seen.add(name);

		const source = join(packageDir, "dist", "package.aospkg");
		if (!existsSync(source)) {
			throw new Error(
				`missing ${relative(repoRoot, source)}; build the complete software catalog before staging`,
			);
		}
		const bytes = readFileSync(source);
		if (bytes.length === 0) throw new Error(`software artifact is empty: ${source}`);
		const digestHex = createHash("sha256").update(bytes).digest("hex");
		const artifactPath = `packages/${name}/${digestHex}.aospkg`;
		const destination = join(outputDir, artifactPath);
		mkdirSync(dirname(destination), { recursive: true });
		cpSync(source, destination);
		artifacts.push({
			name,
			digest: `sha256:${digestHex}`,
			size: statSync(destination).size,
			path: artifactPath,
		});
	}

	if (artifacts.length === 0) {
		throw new Error(`no built software artifacts found under ${softwareRoot}`);
	}
	artifacts.sort((a, b) => a.name.localeCompare(b.name));
	const manifest: SoftwareArtifactManifest = { schemaVersion: 1, artifacts };
	const manifestPath = join(outputDir, "manifest.json");
	writeFileSync(manifestPath, `${JSON.stringify(manifest, null, "\t")}\n`);
	return { manifest, manifestPath, outputDir };
}
