/**
 * `.aospkg` packer — the toolchain half of the canonical packer in
 * `crates/vfs/src/package_format/pack.rs` (both encode the schema in
 * `crates/vfs/package-format/v1.bare`; the TS codecs are generated from it by
 * `pnpm --dir packages/build-tools build:package-format`).
 *
 * Container layout: `16-byte header + vbare PackageManifest + vbare MountIndex
 * + mount.tar` where each vbare chunk is `[u16 LE schema version] ++ BARE`.
 *
 * `agentos-package.json` is a pack-time INPUT only: it is parsed to build the
 * chunk1 vbare manifest and stripped from the packed mount tar. The vbare
 * manifest is the single runtime manifest; nothing re-materializes JSON into
 * the guest. A root `package.json` stays in the mount tar (Node module
 * resolution may need it) and its `bin` map is consulted for command targets.
 */

import { readFileSync, writeFileSync } from "node:fs";
import {
	encodeMountIndex,
	encodePackageManifest,
	TarEntryKind,
	type AgentBlock,
	type CommandTarget,
	type ManPage,
	type PackageManifest,
	type ProvidesBlock,
	type TarEntry,
} from "./generated-package-format.js";

const AOSPKG_MAGIC = Uint8Array.from([0x89, 0x41, 0x4f, 0x53]); // 0x89 'A' 'O' 'S'
const AOSPKG_FORMAT_VERSION = 1;
const PACKAGE_MANIFEST_VERSION = 1;
const MANIFEST_JSON_NAME = "agentos-package.json";
const SNAPSHOT_BUNDLE_PATH = "/dist/sdk-snapshot.js";
const BLOCK = 512;
const S_IFDIR = 0o040000;
const S_IFREG = 0o100000;
const S_IFLNK = 0o120000;

export interface AospkgSummary {
	name: string;
	version: string;
	commands: string[];
}

interface SourceManifestJson {
	name?: string;
	version?: string;
	agent?: {
		acpEntrypoint: string;
		snapshot?: boolean;
		env?: Record<string, string>;
		launchArgs?: string[];
	};
	provides?: {
		env?: Record<string, string>;
		files?: { source: string; target: string }[];
	};
}

interface RawTarMember {
	path: string;
	typeflag: string;
	mode: number;
	uid: number;
	gid: number;
	mtime: number;
	size: number;
	linkTarget?: string;
	/** Record slice in the SOURCE tar: extension headers + header + data blocks. */
	recordStart: number;
	recordEnd: number;
	/** Offset of the member's data (relative to recordStart). */
	dataOffsetInRecord: number;
}

/** Pack `sourceTar` into a `.aospkg` at `dest`. */
export function packAospkgFromTar(
	sourceTar: string,
	dest: string,
	fallbackVersion?: string,
): AospkgSummary {
	const source = readFileSync(sourceTar);
	const { bytes, summary } = packAospkgFromTarBytes(source, fallbackVersion);
	writeFileSync(dest, bytes);
	return summary;
}

export function packAospkgFromTarBytes(
	source: Buffer,
	fallbackVersion?: string,
): { bytes: Buffer; summary: AospkgSummary } {
	const members = parseTarMembers(source);

	// Rebuild the mount tar without agentos-package.json, tracking each kept
	// member's data offset in the OUTPUT so the index refers to the packed tar.
	let manifestJson: Buffer | undefined;
	let packageJson: Buffer | undefined;
	const keptSlices: Buffer[] = [];
	const entries = new Map<string, TarEntry>();
	let outOffset = 0;
	for (const member of members) {
		if (member.path === `/${MANIFEST_JSON_NAME}`) {
			manifestJson = memberData(source, member);
			continue; // stripped: the vbare manifest is the runtime manifest
		}
		if (member.path === "/") {
			continue;
		}
		const indexed = indexEntry(member);
		if (indexed === undefined) {
			continue; // hardlinks/devices/fifos are not part of the package surface
		}
		if (member.path === "/package.json") {
			packageJson = memberData(source, member);
		}
		const record = source.subarray(member.recordStart, member.recordEnd);
		keptSlices.push(record);
		synthesizeParentDirs(member.path, entries);
		entries.set(member.path, {
			...indexed,
			offset: BigInt(outOffset + member.dataOffsetInRecord),
		});
		outOffset += record.length;
	}
	keptSlices.push(Buffer.alloc(2 * BLOCK)); // end-of-archive marker
	const mountTar = Buffer.concat(keptSlices);

	if (!entries.has("/")) {
		entries.set("/", {
			path: "/",
			kind: TarEntryKind.Directory,
			offset: 0n,
			size: 0n,
			mode: S_IFDIR | 0o755,
			uid: 0,
			gid: 0,
			mtime: 0n,
			linkTarget: null,
		});
	}

	if (manifestJson === undefined) {
		throw new Error(`source tar must contain /${MANIFEST_JSON_NAME}`);
	}
	let sourceManifest: SourceManifestJson;
	try {
		sourceManifest = JSON.parse(manifestJson.toString("utf8"));
	} catch (error) {
		throw new Error(`invalid ${MANIFEST_JSON_NAME}: ${String(error)}`);
	}
	const name = sourceManifest.name;
	if (typeof name !== "string" || name.length === 0) {
		throw new Error(`${MANIFEST_JSON_NAME} is missing a valid "name"`);
	}
	const version =
		typeof sourceManifest.version === "string" &&
		sourceManifest.version.length > 0
			? sourceManifest.version
			: fallbackVersion;
	if (typeof version !== "string" || version.length === 0) {
		throw new Error(`${MANIFEST_JSON_NAME} is missing a valid "version"`);
	}

	const sortedPaths = [...entries.keys()].sort();
	const tarEntries = sortedPaths.map((path) => entries.get(path) as TarEntry);
	const commands = commandTargets(sortedPaths, packageJson);
	const manPages = manPagesFromIndex(sortedPaths);
	const agent = agentBlock(sourceManifest);
	const snapshotBundlePath =
		agent?.snapshot && entries.has(SNAPSHOT_BUNDLE_PATH)
			? SNAPSHOT_BUNDLE_PATH
			: null;

	const manifest: PackageManifest = {
		name,
		version,
		agent,
		provides: providesBlock(sourceManifest),
		commands,
		manPages,
		snapshotBundlePath,
	};

	const manifestChunk = versionedChunk(encodePackageManifest(manifest));
	const indexChunk = versionedChunk(encodeMountIndex({ tarEntries }));
	const header = Buffer.alloc(16);
	Buffer.from(AOSPKG_MAGIC).copy(header, 0);
	header.writeUInt16LE(AOSPKG_FORMAT_VERSION, 4);
	header.writeUInt16LE(0, 6);
	header.writeUInt32LE(manifestChunk.length, 8);
	header.writeUInt32LE(indexChunk.length, 12);

	return {
		bytes: Buffer.concat([header, manifestChunk, indexChunk, mountTar]),
		summary: {
			name,
			version,
			commands: commands.map((target) => target.command),
		},
	};
}

function versionedChunk(payload: Uint8Array): Buffer {
	const chunk = Buffer.alloc(2 + payload.length);
	chunk.writeUInt16LE(PACKAGE_MANIFEST_VERSION, 0);
	Buffer.from(payload).copy(chunk, 2);
	return chunk;
}

function agentBlock(manifest: SourceManifestJson): AgentBlock | null {
	const agent = manifest.agent;
	if (agent === undefined) return null;
	return {
		acpEntrypoint: agent.acpEntrypoint,
		snapshot: agent.snapshot ?? false,
		env: new Map(Object.entries(agent.env ?? {})),
		launchArgs: agent.launchArgs ?? [],
	};
}

function providesBlock(manifest: SourceManifestJson): ProvidesBlock | null {
	const provides = manifest.provides;
	if (provides === undefined) return null;
	return {
		env: new Map(Object.entries(provides.env ?? {})),
		files: (provides.files ?? []).map((file) => ({
			source: file.source,
			target: file.target,
		})),
	};
}

function indexEntry(member: RawTarMember): TarEntry | undefined {
	const mode = member.mode & 0o7777;
	const base = {
		path: member.path,
		offset: 0n,
		size: 0n,
		uid: member.uid,
		gid: member.gid,
		mtime: BigInt(member.mtime),
		linkTarget: null as string | null,
	};
	if (member.typeflag === "5") {
		return { ...base, kind: TarEntryKind.Directory, mode: S_IFDIR | mode };
	}
	if (member.typeflag === "2") {
		if (member.linkTarget === undefined) {
			throw new Error(`symlink ${member.path} has no target`);
		}
		return {
			...base,
			kind: TarEntryKind.Symlink,
			mode: S_IFLNK | Math.max(mode, 0o777),
			linkTarget: member.linkTarget,
		};
	}
	if (member.typeflag === "0" || member.typeflag === "\0" || member.typeflag === "7") {
		return {
			...base,
			kind: TarEntryKind.File,
			mode: S_IFREG | mode,
			size: BigInt(member.size),
		};
	}
	return undefined;
}

function synthesizeParentDirs(
	path: string,
	entries: Map<string, TarEntry>,
): void {
	const parts = path.split("/").filter((part) => part.length > 0);
	let current = "";
	for (const part of parts.slice(0, -1)) {
		current += `/${part}`;
		if (!entries.has(current)) {
			entries.set(current, {
				path: current,
				kind: TarEntryKind.Directory,
				offset: 0n,
				size: 0n,
				mode: S_IFDIR | 0o755,
				uid: 0,
				gid: 0,
				mtime: 0n,
				linkTarget: null,
			});
		}
	}
}

function commandTargets(
	sortedPaths: string[],
	packageJson: Buffer | undefined,
): CommandTarget[] {
	if (packageJson !== undefined) {
		try {
			const fromBin = commandTargetsFromPackageJson(
				JSON.parse(packageJson.toString("utf8")),
			);
			if (fromBin !== undefined) return fromBin;
		} catch {
			// fall through to bin/ scan
		}
	}
	return sortedPaths
		.filter(
			(path) =>
				path.startsWith("/bin/") &&
				!path.slice("/bin/".length).includes("/") &&
				isProjectableCommandName(path.slice("/bin/".length)),
		)
		.map((path) => {
			const name = path.slice("/bin/".length);
			return { command: name, entry: `bin/${name}` };
		});
}

function commandTargetsFromPackageJson(value: {
	name?: unknown;
	bin?: unknown;
}): CommandTarget[] | undefined {
	if (typeof value.bin === "string") {
		if (typeof value.name !== "string") return undefined;
		const unscoped = value.name.split("/").pop() ?? value.name;
		return isProjectableCommandName(unscoped)
			? [{ command: unscoped, entry: normalizeRel(value.bin) }]
			: [];
	}
	if (value.bin !== null && typeof value.bin === "object") {
		return Object.entries(value.bin as Record<string, unknown>)
			.filter(
				([name, entry]) =>
					isProjectableCommandName(name) && typeof entry === "string",
			)
			.map(([name, entry]) => ({
				command: name,
				entry: normalizeRel(entry as string),
			}))
			.sort((a, b) => a.command.localeCompare(b.command));
	}
	return undefined;
}

function manPagesFromIndex(sortedPaths: string[]): ManPage[] {
	return sortedPaths
		.flatMap((path) => {
			const suffix = path.startsWith("/share/man/")
				? path.slice("/share/man/".length)
				: undefined;
			if (suffix === undefined) return [];
			const parts = suffix.split("/");
			if (parts.length !== 2) return [];
			return [{ section: parts[0], page: parts[1] }];
		})
		.sort(
			(a, b) =>
				a.section.localeCompare(b.section) || a.page.localeCompare(b.page),
		);
}

function isProjectableCommandName(name: string): boolean {
	return !name.startsWith("_") && !name.startsWith(".");
}

function normalizeRel(path: string): string {
	return path.startsWith("./") ? path.slice(2) : path;
}

// ── minimal tar reader ────────────────────────────────────────────────
// Parses ustar/GNU/pax archives well enough to index a package tar: regular
// members plus 'L' (GNU longname), 'K' (GNU longlink), and 'x' (pax) extension
// records. Extension records travel with their member so record slices can be
// copied verbatim into the repacked mount tar.

function parseTarMembers(source: Buffer): RawTarMember[] {
	const members: RawTarMember[] = [];
	let offset = 0;
	let pendingName: string | undefined;
	let pendingLink: string | undefined;
	let recordStart = 0;
	let sawExtension = false;
	while (offset + BLOCK <= source.length) {
		const block = source.subarray(offset, offset + BLOCK);
		if (isZeroBlock(block)) break;
		if (!sawExtension) {
			recordStart = offset;
		}
		const typeflag = String.fromCharCode(block[156]);
		const size = parseOctal(block, 124, 12);
		const dataBlocks = Math.ceil(size / BLOCK);
		const dataStart = offset + BLOCK;
		const next = dataStart + dataBlocks * BLOCK;
		if (typeflag === "L" || typeflag === "K") {
			const value = source
				.subarray(dataStart, dataStart + size)
				.toString("utf8")
				.replace(/\0+$/, "");
			if (typeflag === "L") pendingName = value;
			else pendingLink = value;
			sawExtension = true;
			offset = next;
			continue;
		}
		if (typeflag === "x" || typeflag === "g") {
			if (typeflag === "x") {
				const records = parsePaxRecords(
					source.subarray(dataStart, dataStart + size),
				);
				if (records.path !== undefined) pendingName = records.path;
				if (records.linkpath !== undefined) pendingLink = records.linkpath;
				sawExtension = true;
			} else {
				// global pax header: applies archive-wide; not copied per-member.
				recordStart = next;
			}
			offset = next;
			continue;
		}
		const rawName = readCString(block, 0, 100);
		const prefix = isUstar(block) ? readCString(block, 345, 155) : "";
		const name =
			pendingName ?? (prefix.length > 0 ? `${prefix}/${rawName}` : rawName);
		const linkTarget =
			pendingLink ??
			(typeflag === "2" ? readCString(block, 157, 100) : undefined);
		members.push({
			path: canonicalTarPath(name),
			typeflag,
			mode: parseOctal(block, 100, 8),
			uid: parseOctal(block, 108, 8),
			gid: parseOctal(block, 116, 8),
			mtime: parseOctal(block, 136, 12),
			size,
			linkTarget,
			recordStart,
			recordEnd: next,
			dataOffsetInRecord: dataStart - recordStart,
		});
		pendingName = undefined;
		pendingLink = undefined;
		sawExtension = false;
		offset = next;
	}
	return members;
}

function memberData(source: Buffer, member: RawTarMember): Buffer {
	const dataStart = member.recordStart + member.dataOffsetInRecord;
	return source.subarray(dataStart, dataStart + member.size);
}

function parsePaxRecords(data: Buffer): { path?: string; linkpath?: string } {
	const out: { path?: string; linkpath?: string } = {};
	let offset = 0;
	const text = data.toString("utf8");
	while (offset < text.length) {
		const space = text.indexOf(" ", offset);
		if (space === -1) break;
		const length = Number.parseInt(text.slice(offset, space), 10);
		if (!Number.isFinite(length) || length <= 0) break;
		const record = text.slice(offset, offset + length);
		const eq = record.indexOf("=");
		if (eq !== -1) {
			const key = record.slice(space - offset + 1, eq);
			const value = record.slice(eq + 1).replace(/\n$/, "");
			if (key === "path") out.path = value;
			if (key === "linkpath") out.linkpath = value;
		}
		offset += length;
	}
	return out;
}

function canonicalTarPath(name: string): string {
	const parts = name
		.split("/")
		.filter((part) => part.length > 0 && part !== ".");
	for (const part of parts) {
		if (part === "..") {
			throw new Error(`tar member path escapes root: ${name}`);
		}
	}
	return parts.length === 0 ? "/" : `/${parts.join("/")}`;
}

function isUstar(block: Buffer): boolean {
	return block.subarray(257, 262).toString("latin1") === "ustar";
}

function isZeroBlock(block: Buffer): boolean {
	return block.every((byte) => byte === 0);
}

function readCString(block: Buffer, start: number, length: number): string {
	const slice = block.subarray(start, start + length);
	const nul = slice.indexOf(0);
	return slice.subarray(0, nul === -1 ? length : nul).toString("utf8");
}

function parseOctal(block: Buffer, start: number, length: number): number {
	const slice = block.subarray(start, start + length);
	// GNU base-256 extension for large numeric fields.
	if ((slice[0] & 0x80) !== 0) {
		let value = slice[0] & 0x7f;
		for (let i = 1; i < length; i += 1) {
			value = value * 256 + slice[i];
		}
		return value;
	}
	const text = slice.toString("latin1").replace(/\0.*$/, "").trim();
	if (text.length === 0) return 0;
	const value = Number.parseInt(text, 8);
	return Number.isFinite(value) ? value : 0;
}
