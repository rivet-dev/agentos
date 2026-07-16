import * as posixPath from "node:path/posix";
import type { VirtualFileSystem } from "./runtime-compat.js";

export interface FilesystemEntry {
	path: string;
	type: "directory" | "file" | "symlink";
	mode: string;
	uid: number;
	gid: number;
	content?: string;
	encoding?: "utf8" | "base64";
	target?: string;
}

export function sortFilesystemEntries(
	entries: FilesystemEntry[],
): FilesystemEntry[] {
	return [...entries].sort((a, b) => {
		const depthA =
			a.path === "/" ? 0 : a.path.split("/").filter(Boolean).length;
		const depthB =
			b.path === "/" ? 0 : b.path.split("/").filter(Boolean).length;

		if (depthA !== depthB) {
			return depthA - depthB;
		}

		return a.path.localeCompare(b.path);
	});
}

function toModeString(mode: number): string {
	return `0${(mode & 0o7777).toString(8)}`;
}

async function snapshotPath(
	filesystem: VirtualFileSystem,
	path: string,
	entries: FilesystemEntry[],
): Promise<void> {
	const stat =
		path === "/" ? await filesystem.stat(path) : await filesystem.lstat(path);

	if (stat.isSymbolicLink) {
		entries.push({
			path,
			type: "symlink",
			mode: toModeString(stat.mode),
			uid: stat.uid,
			gid: stat.gid,
			target: await filesystem.readlink(path),
		});
		return;
	}

	if (stat.isDirectory) {
		entries.push({
			path,
			type: "directory",
			mode: toModeString(stat.mode),
			uid: stat.uid,
			gid: stat.gid,
		});

		const dirEntries = await filesystem.readDirWithTypes(path);
		const children = dirEntries
			.map((entry) => entry.name)
			.filter((name) => name !== "." && name !== "..")
			.sort((a, b) => a.localeCompare(b));

		for (const child of children) {
			const childPath =
				path === "/" ? posixPath.join("/", child) : posixPath.join(path, child);
			await snapshotPath(filesystem, childPath, entries);
		}
		return;
	}

	const content = Buffer.from(await filesystem.readFile(path)).toString(
		"base64",
	);
	entries.push({
		path,
		type: "file",
		mode: toModeString(stat.mode),
		uid: stat.uid,
		gid: stat.gid,
		content,
		encoding: "base64",
	});
}

export async function snapshotVirtualFilesystem(
	filesystem: VirtualFileSystem,
	rootPath = "/",
): Promise<FilesystemEntry[]> {
	const entries: FilesystemEntry[] = [];
	await snapshotPath(filesystem, rootPath, entries);
	return entries;
}
