import { decodeAospkgManifest } from "../../../../packages/agentos-toolchain/src/aospkg.js";
import { decodeMountIndex, TarEntryKind } from "../../../../packages/agentos-toolchain/src/generated-package-format.js";
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
import { basename, dirname, isAbsolute, join, posix, relative, resolve } from "node:path";

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
	/** Manifest entries of the default software, in install order. */
	defaultSoftware: SoftwareArtifactManifest;
	defaultSoftwarePath: string;
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

interface SoftwarePackageManifest {
	commands?: string[];
	aliases?: Record<string, string>;
	stubs?: string[];
	name?: string;
}

function declaredCommands(manifest: SoftwarePackageManifest): string[] {
	return [
		...(manifest.commands ?? []),
		...Object.keys(manifest.aliases ?? {}),
		...(manifest.stubs ?? []),
	];
}

function verifyStagedCommands(packageDir: string, commands: string[]): void {
	const missing = commands.filter((command) => {
		if (!command || command === "." || command === ".." || /[/\\\0]/.test(command)) {
			throw new Error(`invalid declared software command in ${packageDir}: ${command}`);
		}
		const path = join(packageDir, "bin", command);
		return !existsSync(path) || !statSync(path).isFile() || statSync(path).size === 0;
	});
	if (missing.length > 0) {
		throw new Error(
			`missing staged commands in ${packageDir}: ${missing.join(", ")}; run just tools-rebuild before publishing`,
		);
	}
}

/** Validate the package consumers receive, independently of loose staging files. */
function verifyPackedCommands(source: string, bytes: Buffer, commands: string[]): void {
	try {
		const manifest = decodeAospkgManifest(bytes);
		const indexStart = 16 + bytes.readUInt32LE(8);
		const indexLength = bytes.readUInt32LE(12);
		const payloadStart = indexStart + indexLength;
		if (indexLength < 2 || payloadStart >= bytes.length || bytes.readUInt16LE(indexStart) !== bytes.readUInt16LE(16)) {
			throw new Error("invalid .aospkg mount index or payload range");
		}
		const index = decodeMountIndex(bytes.subarray(indexStart + 2, payloadStart));
		const entries = new Map(index.tarEntries.map((entry) => [entry.path, entry]));
		const targets = new Map(manifest.commands.map((command) => [command.command, command.entry]));
		const missing = commands.filter((command) => !targets.has(command));
		if (missing.length > 0) throw new Error(`missing packed commands: ${missing.join(", ")}`);
		for (const [command, target] of targets) {
			let path = posix.resolve("/", target);
			const visited = new Set<string>();
			while (true) {
				if (visited.has(path)) throw new Error(`packed command ${command} has a symlink cycle`);
				visited.add(path);
				const entry = entries.get(path);
				if (!entry) throw new Error(`packed command ${command} is missing payload ${path}`);
				if (entry.kind === TarEntryKind.Symlink && entry.linkTarget !== null) {
					path = posix.resolve(posix.dirname(path), entry.linkTarget);
					continue;
				}
				if (entry.kind !== TarEntryKind.File || entry.size <= 0n || entry.offset + entry.size > BigInt(bytes.length - payloadStart)) {
					throw new Error(`packed command ${command} has an empty or invalid payload ${path}`);
				}
				break;
			}
		}
	} catch (error) {
		throw new Error(`invalid software artifact ${source}: ${error instanceof Error ? error.message : String(error)}; rebuild the package before publishing`, { cause: error });
	}
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
		if (!existsSync(packageManifest)) continue;
		const manifest = JSON.parse(readFileSync(packageManifest, "utf8")) as SoftwarePackageManifest;
		const commands = declaredCommands(manifest);
		if (commands.length === 0 && !manifest.name) continue;
		const name = readPackageIdentity(join(packageDir, "package.json"));
		if (seen.has(name)) throw new Error(`duplicate registry software name: ${name}`);
		seen.add(name);

		const source = join(packageDir, "dist", "package.aospkg");
		if (!existsSync(source)) {
			throw new Error(
				`missing ${relative(repoRoot, source)}; build the complete software catalog before staging`,
			);
		}
		verifyStagedCommands(packageDir, commands);
		const bytes = readFileSync(source);
		if (bytes.length === 0) throw new Error(`software artifact is empty: ${source}`);
		verifyPackedCommands(source, bytes, commands);
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

	const defaultSoftwareNames: string[] = JSON.parse(
		readFileSync(join(softwareRoot, "default-software.json"), "utf8"),
	);
	const defaultSoftware: SoftwareArtifactManifest = {
		schemaVersion: 1,
		artifacts: defaultSoftwareNames.map((name) => {
			const artifact = artifacts.find((candidate) => candidate.name === name);
			if (!artifact) {
				throw new Error(`default software ${name} is not in the software catalog`);
			}
			return artifact;
		}),
	};
	const defaultSoftwarePath = join(outputDir, "default-software.json");
	writeFileSync(
		defaultSoftwarePath,
		`${JSON.stringify(defaultSoftware, null, "\t")}\n`,
	);
	return { manifest, manifestPath, defaultSoftware, defaultSoftwarePath, outputDir };
}
