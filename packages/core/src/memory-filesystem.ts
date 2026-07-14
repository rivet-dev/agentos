import type {
	VirtualDirEntry,
	VirtualFileSystem,
	VirtualStat,
} from "./runtime.js";

const S_IFREG = 0o100000;
const S_IFDIR = 0o040000;
const S_IFLNK = 0o120000;
const MAX_SYMLINK_DEPTH = 40;

export class KernelError extends Error {
	readonly code: string;

	constructor(code: string, message: string) {
		super(message.startsWith(`${code}:`) ? message : `${code}: ${message}`);
		this.name = "KernelError";
		this.code = code;
	}
}

function normalizePath(inputPath: string): string {
	if (!inputPath) return "/";
	let normalized = inputPath.startsWith("/") ? inputPath : `/${inputPath}`;
	normalized = normalized.replace(/\/+/g, "/");
	if (normalized.length > 1 && normalized.endsWith("/")) {
		normalized = normalized.slice(0, -1);
	}
	const parts = normalized.split("/");
	const resolved: string[] = [];
	for (const part of parts) {
		if (part === "" || part === ".") continue;
		if (part === "..") {
			resolved.pop();
			continue;
		}
		resolved.push(part);
	}
	return resolved.length === 0 ? "/" : `/${resolved.join("/")}`;
}

function dirnameVirtual(inputPath: string): string {
	const normalized = normalizePath(inputPath);
	if (normalized === "/") return "/";
	const parts = normalized.split("/").filter(Boolean);
	return parts.length <= 1 ? "/" : `/${parts.slice(0, -1).join("/")}`;
}

interface FileEntry {
	type: "file";
	data: Uint8Array;
	mode: number;
	uid: number;
	gid: number;
	nlink: number;
	ino: number;
	atimeMs: number;
	mtimeMs: number;
	ctimeMs: number;
	birthtimeMs: number;
}

interface DirectoryEntry {
	type: "dir";
	mode: number;
	uid: number;
	gid: number;
	nlink: number;
	ino: number;
	atimeMs: number;
	mtimeMs: number;
	ctimeMs: number;
	birthtimeMs: number;
}

interface SymlinkEntry {
	type: "symlink";
	target: string;
	mode: number;
	uid: number;
	gid: number;
	nlink: number;
	ino: number;
	atimeMs: number;
	mtimeMs: number;
	ctimeMs: number;
	birthtimeMs: number;
}

type MemoryEntry = FileEntry | DirectoryEntry | SymlinkEntry;
let nextInode = 1;

export class InMemoryFileSystem implements VirtualFileSystem {
	private readonly entries = new Map<string, MemoryEntry>();

	constructor() {
		this.entries.set("/", this.newDirectory());
	}

	async readFile(targetPath: string): Promise<Uint8Array> {
		const entry = this.resolveEntry(targetPath);
		if (!entry || entry.type !== "file") {
			throw errnoError("ENOENT", `open '${targetPath}'`);
		}
		entry.atimeMs = Date.now();
		return new Uint8Array(entry.data);
	}

	async readTextFile(targetPath: string): Promise<string> {
		return new TextDecoder().decode(await this.readFile(targetPath));
	}

	async readDir(targetPath: string): Promise<string[]> {
		return (await this.readDirWithTypes(targetPath)).map((entry) => entry.name);
	}

	async readDirWithTypes(targetPath: string): Promise<VirtualDirEntry[]> {
		const resolved = this.resolvePath(targetPath);
		const entry = this.entries.get(resolved);
		if (!entry || entry.type !== "dir") {
			throw errnoError("ENOENT", `scandir '${targetPath}'`);
		}
		const prefix = resolved === "/" ? "/" : `${resolved}/`;
		const output = new Map<string, VirtualDirEntry>();
		for (const [entryPath, candidate] of this.entries) {
			if (!entryPath.startsWith(prefix)) continue;
			const rest = entryPath.slice(prefix.length);
			if (!rest || rest.includes("/")) continue;
			output.set(rest, {
				name: rest,
				isDirectory: candidate.type === "dir",
				isSymbolicLink: candidate.type === "symlink",
			});
		}
		return [...output.values()];
	}

	async writeFile(
		targetPath: string,
		content: string | Uint8Array,
	): Promise<void> {
		const normalized = normalizePath(targetPath);
		await this.mkdir(dirnameVirtual(normalized), { recursive: true });
		const data =
			typeof content === "string"
				? new TextEncoder().encode(content)
				: new Uint8Array(content);
		const existing = this.entries.get(normalized);
		if (existing?.type === "file") {
			existing.data = data;
			existing.mtimeMs = Date.now();
			existing.ctimeMs = Date.now();
			return;
		}
		const now = Date.now();
		this.entries.set(normalized, {
			type: "file",
			data,
			mode: S_IFREG | 0o644,
			uid: 0,
			gid: 0,
			nlink: 1,
			ino: nextInode++,
			atimeMs: now,
			mtimeMs: now,
			ctimeMs: now,
			birthtimeMs: now,
		});
	}

	async createDir(targetPath: string): Promise<void> {
		const normalized = normalizePath(targetPath);
		if (!this.entries.has(dirnameVirtual(normalized))) {
			throw errnoError("ENOENT", `mkdir '${targetPath}'`);
		}
		if (!this.entries.has(normalized)) {
			this.entries.set(normalized, this.newDirectory());
		}
	}

	async mkdir(
		targetPath: string,
		options?: { recursive?: boolean },
	): Promise<void> {
		const normalized = normalizePath(targetPath);
		if (options?.recursive === false) {
			return this.createDir(normalized);
		}
		let current = "";
		for (const part of normalized.split("/").filter(Boolean)) {
			current += `/${part}`;
			if (!this.entries.has(current)) {
				this.entries.set(current, this.newDirectory());
			}
		}
	}

	async exists(targetPath: string): Promise<boolean> {
		try {
			return this.entries.has(this.resolvePath(targetPath));
		} catch {
			return false;
		}
	}

	async stat(targetPath: string): Promise<VirtualStat> {
		const entry = this.resolveEntry(targetPath);
		if (!entry) throw errnoError("ENOENT", `stat '${targetPath}'`);
		return this.toStat(entry);
	}

	async removeFile(targetPath: string): Promise<void> {
		const resolved = normalizePath(targetPath);
		const entry = this.entries.get(resolved);
		if (!entry || entry.type === "dir") {
			throw errnoError("ENOENT", `unlink '${targetPath}'`);
		}
		this.entries.delete(resolved);
	}

	async removeDir(targetPath: string): Promise<void> {
		const resolved = this.resolvePath(targetPath);
		if (resolved === "/") {
			throw errnoError("EPERM", "operation not permitted");
		}
		const entry = this.entries.get(resolved);
		if (!entry || entry.type !== "dir") {
			throw errnoError("ENOENT", `rmdir '${targetPath}'`);
		}
		const prefix = `${resolved}/`;
		for (const key of this.entries.keys()) {
			if (key.startsWith(prefix)) {
				throw errnoError("ENOTEMPTY", `directory not empty '${targetPath}'`);
			}
		}
		this.entries.delete(resolved);
	}

	async rename(oldPath: string, newPath: string): Promise<void> {
		const oldResolved = this.resolvePath(oldPath);
		const newResolved = normalizePath(newPath);
		const entry = this.entries.get(oldResolved);
		if (!entry) throw errnoError("ENOENT", `rename '${oldPath}'`);
		if (!this.entries.has(dirnameVirtual(newResolved))) {
			throw errnoError("ENOENT", `rename '${newPath}'`);
		}
		if (entry.type !== "dir") {
			this.entries.set(newResolved, entry);
			this.entries.delete(oldResolved);
			return;
		}
		const prefix = `${oldResolved}/`;
		const moved: Array<[string, MemoryEntry]> = [];
		for (const candidate of this.entries) {
			if (candidate[0] === oldResolved || candidate[0].startsWith(prefix)) {
				moved.push(candidate);
			}
		}
		for (const [candidatePath] of moved) {
			this.entries.delete(candidatePath);
		}
		for (const [candidatePath, candidate] of moved) {
			const nextPath =
				candidatePath === oldResolved
					? newResolved
					: `${newResolved}${candidatePath.slice(oldResolved.length)}`;
			this.entries.set(nextPath, candidate);
		}
	}

	async realpath(targetPath: string): Promise<string> {
		return this.resolvePath(targetPath);
	}

	async symlink(target: string, linkPath: string): Promise<void> {
		const normalized = normalizePath(linkPath);
		if (this.entries.has(normalized)) {
			throw errnoError("EEXIST", `symlink '${linkPath}'`);
		}
		const now = Date.now();
		this.entries.set(normalized, {
			type: "symlink",
			target,
			mode: S_IFLNK | 0o777,
			uid: 0,
			gid: 0,
			nlink: 1,
			ino: nextInode++,
			atimeMs: now,
			mtimeMs: now,
			ctimeMs: now,
			birthtimeMs: now,
		});
	}

	async readlink(targetPath: string): Promise<string> {
		const normalized = normalizePath(targetPath);
		const entry = this.entries.get(normalized);
		if (!entry || entry.type !== "symlink") {
			throw errnoError("ENOENT", `readlink '${targetPath}'`);
		}
		return entry.target;
	}

	async lstat(targetPath: string): Promise<VirtualStat> {
		const entry = this.entries.get(normalizePath(targetPath));
		if (!entry) throw errnoError("ENOENT", `lstat '${targetPath}'`);
		return this.toStat(entry);
	}

	async link(oldPath: string, newPath: string): Promise<void> {
		const entry = this.resolveEntry(oldPath);
		if (!entry || entry.type !== "file") {
			throw errnoError("ENOENT", `link '${oldPath}'`);
		}
		const normalized = normalizePath(newPath);
		if (this.entries.has(normalized)) {
			throw errnoError("EEXIST", `link '${newPath}'`);
		}
		entry.nlink += 1;
		this.entries.set(normalized, entry);
	}

	async chmod(targetPath: string, mode: number): Promise<void> {
		const entry = this.resolveEntry(targetPath);
		if (!entry) throw errnoError("ENOENT", `chmod '${targetPath}'`);
		const typeBits = mode & 0o170000;
		entry.mode =
			typeBits === 0 ? (entry.mode & 0o170000) | (mode & 0o7777) : mode;
		entry.ctimeMs = Date.now();
	}

	async chown(targetPath: string, uid: number, gid: number): Promise<void> {
		const entry = this.resolveEntry(targetPath);
		if (!entry) throw errnoError("ENOENT", `chown '${targetPath}'`);
		entry.uid = uid;
		entry.gid = gid;
		entry.ctimeMs = Date.now();
	}

	async utimes(
		targetPath: string,
		atime: number,
		mtime: number,
	): Promise<void> {
		const entry = this.resolveEntry(targetPath);
		if (!entry) throw errnoError("ENOENT", `utimes '${targetPath}'`);
		entry.atimeMs = atime;
		entry.mtimeMs = mtime;
		entry.ctimeMs = Date.now();
	}

	async truncate(targetPath: string, length: number): Promise<void> {
		const entry = this.resolveEntry(targetPath);
		if (!entry || entry.type !== "file") {
			throw errnoError("ENOENT", `truncate '${targetPath}'`);
		}
		if (length < entry.data.length) {
			entry.data = entry.data.slice(0, length);
		} else if (length > entry.data.length) {
			const expanded = new Uint8Array(length);
			expanded.set(entry.data);
			entry.data = expanded;
		}
		entry.mtimeMs = Date.now();
		entry.ctimeMs = Date.now();
	}

	async pread(
		targetPath: string,
		offset: number,
		length: number,
	): Promise<Uint8Array> {
		const entry = this.resolveEntry(targetPath);
		if (!entry || entry.type !== "file") {
			throw errnoError("ENOENT", `open '${targetPath}'`);
		}
		if (offset >= entry.data.length) return new Uint8Array(0);
		return entry.data.slice(
			offset,
			Math.min(offset + length, entry.data.length),
		);
	}

	async pwrite(
		targetPath: string,
		offset: number,
		data: Uint8Array,
	): Promise<void> {
		const entry = this.resolveEntry(targetPath);
		if (!entry || entry.type !== "file") {
			throw errnoError("ENOENT", `open '${targetPath}'`);
		}
		const nextSize = Math.max(entry.data.length, offset + data.length);
		const updated = new Uint8Array(nextSize);
		updated.set(entry.data);
		updated.set(new Uint8Array(data), offset);
		entry.data = updated;
		entry.mtimeMs = Date.now();
		entry.ctimeMs = Date.now();
	}

	private resolvePath(targetPath: string, depth = 0): string {
		if (depth > MAX_SYMLINK_DEPTH) {
			throw errnoError("ELOOP", `too many symbolic links '${targetPath}'`);
		}
		const normalized = normalizePath(targetPath);
		const entry = this.entries.get(normalized);
		if (!entry) return normalized;
		if (entry.type === "symlink") {
			const target = entry.target.startsWith("/")
				? entry.target
				: `${dirnameVirtual(normalized)}/${entry.target}`;
			return this.resolvePath(target, depth + 1);
		}
		return normalized;
	}

	private resolveEntry(targetPath: string): MemoryEntry | undefined {
		return this.entries.get(this.resolvePath(targetPath));
	}

	private newDirectory(): DirectoryEntry {
		const now = Date.now();
		return {
			type: "dir",
			mode: S_IFDIR | 0o755,
			uid: 0,
			gid: 0,
			nlink: 2,
			ino: nextInode++,
			atimeMs: now,
			mtimeMs: now,
			ctimeMs: now,
			birthtimeMs: now,
		};
	}

	private toStat(entry: MemoryEntry): VirtualStat {
		const size = entry.type === "file" ? entry.data.length : 4096;
		return {
			mode: entry.mode,
			size,
			blocks: size === 0 ? 0 : Math.ceil(size / 512),
			dev: 1,
			rdev: 0,
			isDirectory: entry.type === "dir",
			isSymbolicLink: entry.type === "symlink",
			atimeMs: entry.atimeMs,
			mtimeMs: entry.mtimeMs,
			ctimeMs: entry.ctimeMs,
			birthtimeMs: entry.birthtimeMs,
			ino: entry.ino,
			nlink: entry.nlink,
			uid: entry.uid,
			gid: entry.gid,
		};
	}
}

export function createInMemoryFileSystem(): InMemoryFileSystem {
	return new InMemoryFileSystem();
}
function errnoError(code: string, message: string): KernelError {
	return new KernelError(code, message);
}
