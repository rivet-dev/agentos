import type {
	NetworkAdapter,
	Permissions,
	SystemDriver,
	VirtualFileSystem,
} from "./runtime.js";
import {
	createCommandExecutorStub,
	createEnosysError,
	createFsStub,
	createInMemoryFileSystem,
	createNetworkStub,
	wrapFileSystem,
	wrapNetworkAdapter,
} from "./runtime.js";

const S_IFREG = 0o100000;
const S_IFDIR = 0o040000;

/**
 * Captured reference to the platform `fetch` taken at module load, before any
 * guest code runs and before the worker shadows the guest-visible `fetch`
 * global (see worker.ts dangerousApis lockdown, F-012). The gated network
 * adapter must keep working after the ambient `fetch` global is removed for the
 * guest, so the adapter calls this private reference instead of the (now
 * shadowed) global identifier. Bound to `globalThis` to preserve the correct
 * `this` for the platform fetch implementation.
 */
const platformFetch: typeof fetch | undefined =
	typeof fetch === "function" ? fetch.bind(globalThis) : undefined;

const BROWSER_SYSTEM_DRIVER_OPTIONS = Symbol.for(
	"agent-os.browserSystemDriverOptions",
);

export interface BrowserRuntimeSystemOptions {
	filesystem: "opfs" | "memory";
	networkEnabled: boolean;
}

type BrowserSystemDriver = SystemDriver & {
	[BROWSER_SYSTEM_DRIVER_OPTIONS]?: BrowserRuntimeSystemOptions;
};

function normalizePath(path: string): string {
	if (!path) return "/";
	let normalized = path.startsWith("/") ? path : `/${path}`;
	normalized = normalized.replace(/\/+/g, "/");
	if (normalized.length > 1 && normalized.endsWith("/")) {
		normalized = normalized.slice(0, -1);
	}
	return normalized;
}

function splitPath(path: string): string[] {
	const normalized = normalizePath(path);
	return normalized === "/" ? [] : normalized.slice(1).split("/");
}

function dirname(path: string): string {
	const parts = splitPath(path);
	if (parts.length <= 1) return "/";
	return `/${parts.slice(0, -1).join("/")}`;
}

/**
 * OPFS subdirectory under which every namespaced runtime root is created. The
 * origin-wide OPFS root (`navigator.storage.getDirectory()`) is shared by all
 * runtimes on the origin, so writing a runtime's files directly to it lets
 * co-resident runtimes read each other's data (F-015). Each runtime instead
 * gets its own subdirectory under this prefix.
 */
const OPFS_RUNTIME_NAMESPACE_ROOT = ".agentos-runtimes";

/**
 * Translate an OPFS "not found" error into a Node-style ENOENT error so missing
 * paths report consistently with the in-memory filesystem (and so callers see
 * ENOENT rather than a raw DOMException). Non-not-found errors pass through
 * unchanged to preserve existing behavior.
 */
function toEnoent(error: unknown, op: string, path: string): unknown {
	const name = (error as { name?: string } | undefined)?.name;
	if (name === "NotFoundError") {
		const enoent = new Error(
			`ENOENT: no such file or directory, ${op} '${path}'`,
		);
		(enoent as { code?: string }).code = "ENOENT";
		return enoent;
	}
	return error;
}

/** Generate a unique, stable-for-this-instance OPFS namespace id. */
function generateOpfsNamespace(): string {
	const cryptoObj = (globalThis as { crypto?: Crypto }).crypto;
	if (cryptoObj && typeof cryptoObj.randomUUID === "function") {
		return cryptoObj.randomUUID();
	}
	return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

async function getOriginRootHandle(): Promise<FileSystemDirectoryHandle> {
	if (!("storage" in navigator) || !("getDirectory" in navigator.storage)) {
		throw createEnosysError("opfs");
	}
	return navigator.storage.getDirectory();
}

/**
 * Resolve the per-runtime OPFS root, namespaced under
 * `OPFS_RUNTIME_NAMESPACE_ROOT/<namespace>` so that two runtimes on the same
 * origin never share storage (F-015). All of a runtime's paths resolve under
 * this handle, so its own reads/writes work and persist for its lifetime while
 * remaining invisible to other tenants.
 */
async function getRootHandle(
	namespace: string,
): Promise<FileSystemDirectoryHandle> {
	const originRoot = await getOriginRootHandle();
	const namespaceContainer = await originRoot.getDirectoryHandle(
		OPFS_RUNTIME_NAMESPACE_ROOT,
		{ create: true },
	);
	return namespaceContainer.getDirectoryHandle(namespace, { create: true });
}

async function getNamespaceContainerHandle(
	create = false,
): Promise<FileSystemDirectoryHandle> {
	const originRoot = await getOriginRootHandle();
	return originRoot.getDirectoryHandle(OPFS_RUNTIME_NAMESPACE_ROOT, {
		create,
	});
}

function isNotFoundError(error: unknown): boolean {
	return (error as { name?: string } | undefined)?.name === "NotFoundError";
}

export async function releaseOpfsNamespace(namespace: string): Promise<void> {
	try {
		const namespaceContainer = await getNamespaceContainerHandle(false);
		await namespaceContainer.removeEntry(namespace, { recursive: true });
	} catch (error) {
		if (isNotFoundError(error)) {
			return;
		}
		throw error;
	}
}

export async function listOpfsNamespaces(): Promise<string[]> {
	try {
		const namespaceContainer = await getNamespaceContainerHandle(false);
		const namespaces: string[] = [];
		for await (const [name, handle] of namespaceContainer.entries()) {
			if (handle.kind === "directory") {
				namespaces.push(name);
			}
		}
		return namespaces.sort();
	} catch (error) {
		if (isNotFoundError(error)) {
			return [];
		}
		throw error;
	}
}

/**
 * VFS backed by the Origin Private File System (OPFS) API. Falls back to
 * InMemoryFileSystem when OPFS is unavailable. Rename is not supported
 * (throws ENOSYS) since OPFS doesn't provide atomic rename.
 */
export class OpfsFileSystem implements VirtualFileSystem {
	private rootPromise: Promise<FileSystemDirectoryHandle>;
	readonly namespace: string;

	/**
	 * @param namespace Unique per-runtime/tenant OPFS namespace. Defaults to a
	 * freshly generated id so co-resident runtimes never share storage (F-015).
	 * Pass a stable id to persist a runtime's data across instances.
	 */
	constructor(namespace: string = generateOpfsNamespace()) {
		this.namespace = namespace;
		this.rootPromise = getRootHandle(namespace);
	}

	private async getDirHandle(
		path: string,
		create = false,
	): Promise<FileSystemDirectoryHandle> {
		const root = await this.rootPromise;
		const parts = splitPath(path);
		let current = root;
		for (const part of parts) {
			try {
				current = await current.getDirectoryHandle(part, { create });
			} catch (error) {
				throw toEnoent(error, "stat", normalizePath(path));
			}
		}
		return current;
	}

	private async getFileHandle(
		path: string,
		create = false,
	): Promise<FileSystemFileHandle> {
		const normalized = normalizePath(path);
		const parent = dirname(normalized);
		const name = normalized.split("/").pop() || "";
		const dir = await this.getDirHandle(parent, create);
		try {
			return await dir.getFileHandle(name, { create });
		} catch (error) {
			throw toEnoent(error, "open", normalized);
		}
	}

	async readFile(path: string): Promise<Uint8Array> {
		const handle = await this.getFileHandle(path);
		const file = await handle.getFile();
		const buffer = await file.arrayBuffer();
		return new Uint8Array(buffer);
	}

	async readTextFile(path: string): Promise<string> {
		const handle = await this.getFileHandle(path);
		const file = await handle.getFile();
		return file.text();
	}

	async readDir(path: string): Promise<string[]> {
		const dir = await this.getDirHandle(path);
		const entries: string[] = [];
		for await (const [name] of dir.entries()) {
			entries.push(name);
		}
		return entries;
	}

	async readDirWithTypes(
		path: string,
	): Promise<Array<{ name: string; isDirectory: boolean }>> {
		const dir = await this.getDirHandle(path);
		const entries: Array<{ name: string; isDirectory: boolean }> = [];
		for await (const [name, handle] of dir.entries()) {
			entries.push({
				name,
				isDirectory: handle.kind === "directory",
			});
		}
		return entries;
	}

	async writeFile(path: string, content: string | Uint8Array): Promise<void> {
		const normalized = normalizePath(path);
		await this.mkdir(dirname(normalized));
		const handle = await this.getFileHandle(normalized, true);
		const writable = await handle.createWritable();
		if (typeof content === "string") {
			await writable.write(content);
		} else {
			await writable.write(content as unknown as FileSystemWriteChunkType);
		}
		await writable.close();
	}

	async createDir(path: string): Promise<void> {
		const normalized = normalizePath(path);
		const parent = dirname(normalized);
		await this.getDirHandle(parent, false);
		await this.getDirHandle(normalized, true);
	}

	async mkdir(path: string, _options?: { recursive?: boolean }): Promise<void> {
		const parts = splitPath(path);
		let current = "";
		for (const part of parts) {
			current += `/${part}`;
			await this.getDirHandle(current, true);
		}
	}

	async exists(path: string): Promise<boolean> {
		try {
			await this.getFileHandle(path);
			return true;
		} catch {
			try {
				await this.getDirHandle(path);
				return true;
			} catch {
				return false;
			}
		}
	}

	async stat(path: string) {
		try {
			const handle = await this.getFileHandle(path);
			const file = await handle.getFile();
			return {
				mode: S_IFREG | 0o644,
				size: file.size,
				blocks: file.size === 0 ? 0 : Math.ceil(file.size / 512),
				dev: 1,
				rdev: 0,
				isDirectory: false,
				isSymbolicLink: false,
				atimeMs: file.lastModified,
				mtimeMs: file.lastModified,
				ctimeMs: file.lastModified,
				birthtimeMs: file.lastModified,
				ino: 0,
				nlink: 1,
				uid: 0,
				gid: 0,
			};
		} catch {
			const normalized = normalizePath(path);
			try {
				await this.getDirHandle(normalized);
				const now = Date.now();
				return {
					mode: S_IFDIR | 0o755,
					size: 4096,
					blocks: 8,
					dev: 1,
					rdev: 0,
					isDirectory: true,
					isSymbolicLink: false,
					atimeMs: now,
					mtimeMs: now,
					ctimeMs: now,
					birthtimeMs: now,
					ino: 0,
					nlink: 2,
					uid: 0,
					gid: 0,
				};
			} catch {
				throw new Error(
					`ENOENT: no such file or directory, stat '${normalized}'`,
				);
			}
		}
	}

	async removeFile(path: string): Promise<void> {
		const normalized = normalizePath(path);
		const parent = dirname(normalized);
		const name = normalized.split("/").pop() || "";
		const dir = await this.getDirHandle(parent);
		await dir.removeEntry(name);
	}

	async removeDir(path: string): Promise<void> {
		const normalized = normalizePath(path);
		if (normalized === "/") {
			throw new Error("EPERM: operation not permitted, rmdir '/'");
		}
		const parent = dirname(normalized);
		const name = normalized.split("/").pop() || "";
		const dir = await this.getDirHandle(parent);
		await dir.removeEntry(name);
	}

	async rename(_oldPath: string, _newPath: string): Promise<void> {
		throw createEnosysError("rename");
	}

	async symlink(_target: string, _linkPath: string): Promise<void> {
		throw createEnosysError("symlink");
	}

	async readlink(_path: string): Promise<string> {
		throw createEnosysError("readlink");
	}

	async lstat(path: string) {
		return this.stat(path);
	}

	async link(_oldPath: string, _newPath: string): Promise<void> {
		throw createEnosysError("link");
	}

	async chmod(_path: string, _mode: number): Promise<void> {
		// No-op: OPFS does not support POSIX permissions
	}

	async chown(_path: string, _uid: number, _gid: number): Promise<void> {
		// No-op: OPFS does not support POSIX ownership
	}

	async utimes(_path: string, _atime: number, _mtime: number): Promise<void> {
		// No-op: OPFS does not support timestamp manipulation
	}

	async truncate(path: string, length: number): Promise<void> {
		const handle = await this.getFileHandle(path);
		const writable = await handle.createWritable({ keepExistingData: true });
		await writable.truncate(length);
		await writable.close();
	}

	async realpath(path: string): Promise<string> {
		const normalized = normalizePath(path);
		if (await this.exists(normalized)) return normalized;
		throw new Error(
			`ENOENT: no such file or directory, realpath '${normalized}'`,
		);
	}

	async pread(
		path: string,
		offset: number,
		length: number,
	): Promise<Uint8Array> {
		const data = await this.readFile(path);
		return data.slice(offset, offset + length);
	}

	async pwrite(path: string, offset: number, data: Uint8Array): Promise<void> {
		const content = await this.readFile(path);
		const endPos = offset + data.length;
		const newContent = new Uint8Array(Math.max(content.length, endPos));
		newContent.set(content);
		newContent.set(data, offset);
		await this.writeFile(path, newContent);
	}
}

export interface BrowserDriverOptions {
	filesystem?: "opfs" | "memory";
	permissions?: Permissions;
	useDefaultNetwork?: boolean;
	/**
	 * Per-runtime/tenant OPFS namespace. Defaults to a freshly generated id so
	 * co-resident runtimes never share storage (F-015). Pass a stable id to
	 * persist a runtime's data across driver instances. Generated default
	 * namespaces are not garbage-collected automatically; long-lived embedders
	 * should either pass a stable namespace or call `releaseOpfsNamespace`.
	 */
	opfsNamespace?: string;
}

/**
 * Create an OPFS-backed filesystem, falling back to in-memory if OPFS is
 * unavailable. Each instance is namespaced under a unique per-runtime
 * subdirectory (F-015); pass `namespace` to use a stable, persistent namespace.
 */
export async function createOpfsFileSystem(
	namespace?: string,
): Promise<VirtualFileSystem> {
	if (
		!("storage" in navigator) ||
		typeof navigator.storage.getDirectory !== "function"
	) {
		return createInMemoryFileSystem();
	}
	return new OpfsFileSystem(namespace);
}

export interface BrowserNetworkAdapterOptions {
	fetch?: typeof fetch;
}

/** Network adapter that delegates to the browser's native `fetch`. DNS and http2 are unsupported. */
export function createBrowserNetworkAdapter(
	options: BrowserNetworkAdapterOptions = {},
): NetworkAdapter {
	// Use the reference captured at module load (platformFetch) so the gated
	// adapter keeps working after worker.ts shadows the guest-visible `fetch`
	// global (F-012). Fall back to the current global only if none was captured.
	const fetchImpl: typeof fetch =
		options.fetch ??
		platformFetch ??
		((...args: Parameters<typeof fetch>) => fetch(...args));
	return {
		async fetch(url, options) {
			const response = await fetchImpl(url, {
				method: options?.method || "GET",
				headers: options?.headers,
				body: options?.body as RequestInit["body"],
				// Untrusted guest requests must never ride the embedding page's
				// ambient cookies / HTTP-auth (F-008). Omit all credentials.
				credentials: "omit",
			});
			const headers: Record<string, string> = {};
			response.headers.forEach((v, k) => {
				headers[k] = v;
			});

			const contentType = response.headers.get("content-type") || "";
			const isBinary =
				contentType.includes("octet-stream") ||
				contentType.includes("gzip") ||
				url.endsWith(".tgz");

			let body: string;
			if (isBinary) {
				const buffer = await response.arrayBuffer();
				body = btoa(String.fromCharCode(...new Uint8Array(buffer)));
				headers["x-body-encoding"] = "base64";
			} else {
				body = await response.text();
			}

			return {
				ok: response.ok,
				status: response.status,
				statusText: response.statusText,
				headers,
				body,
				url: response.url,
				redirected: response.redirected,
			};
		},

		async dnsLookup(_hostname) {
			return { error: "DNS not supported in browser", code: "ENOSYS" };
		},

		async httpRequest(url, options) {
			const response = await fetchImpl(url, {
				method: options?.method || "GET",
				headers: options?.headers,
				body: options?.body as RequestInit["body"],
				// Untrusted guest requests must never ride the embedding page's
				// ambient cookies / HTTP-auth (F-008). Omit all credentials.
				credentials: "omit",
			});
			const headers: Record<string, string> = {};
			response.headers.forEach((v, k) => {
				headers[k] = v;
			});
			const body = await response.text();
			return {
				status: response.status,
				statusText: response.statusText,
				headers,
				body,
				url: response.url,
			};
		},
	};
}

/** Recover runtime-driver options from a browser SystemDriver instance. */
export function getBrowserSystemDriverOptions(
	systemDriver: SystemDriver,
): BrowserRuntimeSystemOptions {
	const options = (systemDriver as BrowserSystemDriver)[
		BROWSER_SYSTEM_DRIVER_OPTIONS
	];
	if (options) {
		return options;
	}
	return {
		filesystem: "opfs",
		networkEnabled: Boolean(systemDriver.network),
	};
}

/** Assemble a browser-side SystemDriver with permission-wrapped adapters. */
export async function createBrowserDriver(
	options: BrowserDriverOptions = {},
): Promise<SystemDriver> {
	const permissions = options.permissions;
	const filesystemMode = options.filesystem ?? "opfs";
	const filesystem =
		filesystemMode === "memory"
			? createInMemoryFileSystem()
			: await createOpfsFileSystem(options.opfsNamespace);
	const networkAdapter = options.useDefaultNetwork
		? wrapNetworkAdapter(createBrowserNetworkAdapter(), permissions)
		: undefined;

	const systemDriver: BrowserSystemDriver = {
		filesystem: wrapFileSystem(filesystem, permissions),
		network: networkAdapter,
		commandExecutor: createCommandExecutorStub(),
		permissions,
		runtime: {
			process: {},
			os: {},
		},
	};

	systemDriver[BROWSER_SYSTEM_DRIVER_OPTIONS] = {
		filesystem: filesystemMode,
		networkEnabled: Boolean(networkAdapter),
	};

	return systemDriver;
}

export { createCommandExecutorStub, createFsStub, createNetworkStub };
