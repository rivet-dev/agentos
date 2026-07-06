import {
	createInMemoryFileSystem,
	InMemoryFileSystem,
} from "./os-filesystem.js";
import { guestEncodingBootstrapCode } from "./encoding.js";
import { BROWSER_WASI_POLYFILL_CODE } from "./wasi-polyfill.js";
import { PROCESS_SIGNAL_NUMBERS } from "./signals.js";
import { BROWSER_BUFFER_POLYFILL_CODE } from "./generated/buffer-polyfill.js";
import { BROWSER_PATH_POLYFILL_CODE } from "./generated/path-polyfill.js";
import { BROWSER_UTIL_POLYFILL_CODE } from "./generated/util-polyfill.js";

export type StdioChannel = "stdout" | "stderr";
export type TimingMitigation = "off" | "freeze";
type BodyLike = unknown;

export interface VirtualDirEntry {
	name: string;
	isDirectory: boolean;
	isSymbolicLink?: boolean;
}

export interface VirtualStat {
	mode: number;
	size: number;
	blocks: number;
	dev: number;
	rdev: number;
	isDirectory: boolean;
	isSymbolicLink: boolean;
	atimeMs: number;
	mtimeMs: number;
	ctimeMs: number;
	birthtimeMs: number;
	ino: number;
	nlink: number;
	uid: number;
	gid: number;
}

export interface VirtualFileSystem {
	readFile(path: string): Promise<Uint8Array>;
	readTextFile(path: string): Promise<string>;
	readDir(path: string): Promise<string[]>;
	readDirWithTypes(path: string): Promise<VirtualDirEntry[]>;
	writeFile(path: string, content: string | Uint8Array): Promise<void>;
	createDir(path: string): Promise<void>;
	mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
	exists(path: string): Promise<boolean>;
	stat(path: string): Promise<VirtualStat>;
	removeFile(path: string): Promise<void>;
	removeDir(path: string): Promise<void>;
	rename(oldPath: string, newPath: string): Promise<void>;
	realpath(path: string): Promise<string>;
	symlink(target: string, linkPath: string): Promise<void>;
	readlink(path: string): Promise<string>;
	lstat(path: string): Promise<VirtualStat>;
	link(oldPath: string, newPath: string): Promise<void>;
	chmod(path: string, mode: number): Promise<void>;
	chown(path: string, uid: number, gid: number): Promise<void>;
	utimes(path: string, atime: number, mtime: number): Promise<void>;
	truncate(path: string, length: number): Promise<void>;
	pread(path: string, offset: number, length: number): Promise<Uint8Array>;
	pwrite(path: string, offset: number, data: Uint8Array): Promise<void>;
}

export type PermissionDecision =
	| boolean
	| { allowed: boolean; reason?: string }
	| { allow: boolean; reason?: string };
export type PermissionCheck<T = unknown> = (request: T) => PermissionDecision;

export interface Permissions {
	fs?: PermissionCheck<{ path: string; operation: string }>;
	network?: PermissionCheck<{ url?: string; host?: string; port?: number }>;
	childProcess?: PermissionCheck<{
		command: string;
		args: string[];
		cwd?: string;
		env?: Record<string, string>;
	}>;
	env?: PermissionCheck<{ name: string; value: string }>;
}

export const allowAllFs: PermissionCheck = () => true;
export const allowAllNetwork: PermissionCheck = () => true;
export const allowAllChildProcess: PermissionCheck = () => true;
export const allowAllEnv: PermissionCheck = () => true;
export const allowAll: Permissions = {
	fs: allowAllFs,
	network: allowAllNetwork,
	childProcess: allowAllChildProcess,
	env: allowAllEnv,
};

export interface ExecOptions {
	filePath?: string;
	env?: Record<string, string>;
	cwd?: string;
	stdin?: string;
	/**
	 * Bind process stdio to a kernel PTY slave fd. When present, stdin/stdout/stderr
	 * report isTTY=true, stdout/stderr write through `pty.write(slaveFd, ...)`, and
	 * stdin is read from `pty.read(slaveFd, ...)`.
	 */
	stdioPty?: {
		/**
		 * Open a fresh kernel PTY for this execution before guest code runs. The
		 * execution's stdio binds to the slave; `onOpen` receives the master fd so
		 * the host terminal can drive it via `writePty`/`readPty`.
		 */
		open?: boolean;
		/** Bind to an existing slave fd already owned by this execution. */
		slaveFd?: number;
		columns?: number;
		rows?: number;
		onOpen?: (pty: PtyOpenResult) => void;
	};
	cpuTimeLimitMs?: number;
	timingMitigation?: TimingMitigation;
	onStdio?: StdioHook;
	/**
	 * Run a PERSISTENT (service-style) program: instead of returning once `main`
	 * finishes and microtasks drain, the executor keeps the worker event loop alive
	 * for async I/O (stdin events, timers, stream pumps) until the program calls
	 * `process.exit` (or a safety timeout). Needed for long-running stdio servers such
	 * as an ACP agent that reads requests and replies asynchronously. Default false
	 * (run-to-completion).
	 */
	persistent?: boolean;
	/**
	 * Stream stdin: the host feeds stdin incrementally (`writeStdin`) and ends it
	 * explicitly (`endStdin`) rather than the one-shot `stdin` string that auto-ends.
	 * Pairs with `persistent` to drive a long-running stdio program (e.g. an ACP agent)
	 * as a proper external client. Use `onStart` to learn the execution id.
	 */
	streamingStdin?: boolean;
	/** Called once with the execution id when the run starts (for writeStdin/endStdin). */
	onStart?: (executionId: string) => void;
}

export interface ExecResult {
	code: number;
	exitCode?: number;
	stdout?: string;
	stderr?: string;
	errorMessage?: string;
}

export interface PtyOpenResult {
	masterFd: number;
	slaveFd: number;
	path?: string;
	columns: number;
	rows: number;
}

export interface RunResult<T = unknown> {
	value?: T;
	code: number;
	errorMessage?: string;
	exports?: T;
}

export interface OSConfig {
	platform?: string;
	arch?: string;
	type?: string;
	release?: string;
	version?: string;
	cpuCount?: number;
	totalmem?: number;
	freemem?: number;
	hostname?: string;
	homedir?: string;
	tmpdir?: string;
	machine?: string;
	user?: string;
	shell?: string;
	uid?: number;
	gid?: number;
}

export interface ProcessConfig {
	cwd?: string;
	env?: Record<string, string>;
	argv?: string[];
	platform?: string;
	arch?: string;
	version?: string;
	pid?: number;
	ppid?: number;
	uid?: number;
	gid?: number;
	timingMitigation?: TimingMitigation;
	frozenTimeMs?: number;
}

export type StdioEvent = { channel: StdioChannel; message: string };
export type StdioHook = (event: StdioEvent) => void;

export interface CommandExecutor {
	spawn(
		command: string,
		args: string[],
		options?: {
			cwd?: string;
			env?: Record<string, string>;
			onStdout?: (data: Uint8Array) => void;
			onStderr?: (data: Uint8Array) => void;
		},
	): {
		wait(): Promise<number>;
		writeStdin(data: Uint8Array | string): void;
		closeStdin(): void;
		kill(signal?: number): void;
	};
}

export interface NetworkAdapter {
	fetch(
		url: string,
		options?: {
			method?: string;
			headers?: Record<string, string>;
			body?: BodyLike | null;
		},
	): Promise<{
		ok: boolean;
		status: number;
		statusText: string;
		headers: Record<string, string>;
		body: string;
		url: string;
		redirected: boolean;
	}>;
	dnsLookup(hostname: string): Promise<{
		address?: string;
		family?: number;
		error?: string;
		code?: string;
	}>;
	httpRequest(
		url: string,
		options?: {
			method?: string;
			headers?: Record<string, string>;
			body?: BodyLike | null;
		},
	): Promise<{
		status: number;
		statusText: string;
		headers: Record<string, string>;
		body: string;
		url: string;
	}>;
}

export interface SystemDriver {
	filesystem?: VirtualFileSystem;
	network?: NetworkAdapter;
	commandExecutor?: CommandExecutor;
	permissions?: Permissions;
	runtime: {
		process: ProcessConfig;
		os: OSConfig;
	};
}

export interface RuntimeDriverOptions {
	system: SystemDriver;
	runtime: {
		process: ProcessConfig;
		os: OSConfig;
	};
	memoryLimit?: number;
	cpuTimeLimitMs?: number;
	timingMitigation?: TimingMitigation;
	onStdio?: StdioHook;
	payloadLimits?: {
		base64TransferBytes?: number;
		jsonPayloadBytes?: number;
	};
}

export interface NodeRuntimeDriver {
	exec(code: string, options?: ExecOptions): Promise<ExecResult>;
	run<T = unknown>(code: string, filePath?: string): Promise<RunResult<T>>;
	dispose(): void;
	terminate?(): Promise<void>;
	/** Feed stdin to a running `streamingStdin` execution (id from ExecOptions.onStart). */
	writeStdin?(executionId: string, data: string): void;
	/** End stdin for a running `streamingStdin` execution. */
	endStdin?(executionId: string): void;
	/** Write bytes to a PTY fd owned by the execution. */
	writePty?(executionId: string, fd: number, data: string | Uint8Array): Promise<number>;
	/** Read bytes from a PTY fd owned by the execution. */
	readPty?(
		executionId: string,
		fd: number,
		options?: { maxBytes?: number; timeoutMs?: number },
	): Promise<Uint8Array | null>;
	/** Resize a PTY owned by the execution. */
	resizePty?(
		executionId: string,
		fd: number,
		size: { columns: number; rows: number },
	): Promise<void>;
	/** Close a PTY fd owned by the execution. */
	closePty?(executionId: string, fd: number): Promise<void>;
}

export interface NodeRuntimeDriverFactory {
	createRuntimeDriver(options: RuntimeDriverOptions): NodeRuntimeDriver;
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
		if (!part || part === ".") continue;
		if (part === "..") {
			resolved.pop();
			continue;
		}
		resolved.push(part);
	}
	return resolved.length === 0 ? "/" : `/${resolved.join("/")}`;
}

function dirname(inputPath: string): string {
	const normalized = normalizePath(inputPath);
	if (normalized === "/") return "/";
	const parts = normalized.split("/").filter(Boolean);
	return parts.length <= 1 ? "/" : `/${parts.slice(0, -1).join("/")}`;
}

export function permissionAllowed(
	decision: PermissionDecision | undefined,
): boolean {
	if (decision === undefined) return true;
	if (typeof decision === "boolean") return decision;
	return "allowed" in decision ? decision.allowed : decision.allow;
}

export function filterEnv(
	env: Record<string, string> | undefined,
	permissions?: Permissions,
): Record<string, string> {
	const source = env ?? {};
	if (!permissions?.env) return { ...source };
	const output: Record<string, string> = {};
	for (const [name, value] of Object.entries(source)) {
		if (permissionAllowed(permissions.env({ name, value }))) {
			output[name] = value;
		}
	}
	return output;
}

export function createEnosysError(operation: string): Error {
	const error = new Error(`ENOSYS: ${operation} is not supported`);
	(error as { code?: string }).code = "ENOSYS";
	return error;
}

export function createFsStub(): VirtualFileSystem {
	return createInMemoryFileSystem();
}

export function createNetworkStub(): NetworkAdapter {
	return {
		async fetch() {
			throw createEnosysError("network.fetch");
		},
		async dnsLookup() {
			return { error: "DNS not supported", code: "ENOSYS" };
		},
		async httpRequest() {
			throw createEnosysError("network.httpRequest");
		},
	};
}

export function createCommandExecutorStub(): CommandExecutor {
	return {
		spawn() {
			throw createEnosysError("child_process.spawn");
		},
	};
}

export function wrapCommandExecutor(
	adapter: CommandExecutor,
	permissions?: Permissions,
): CommandExecutor {
	if (!permissions?.childProcess) return adapter;
	const check = (
		command: string,
		args: string[],
		options?: { cwd?: string; env?: Record<string, string> },
	): void => {
		if (
			!permissionAllowed(
				permissions.childProcess?.({
					command,
					args,
					cwd: options?.cwd,
					env: options?.env,
				}),
			)
		) {
			const error = new Error(
				`EACCES: blocked child_process spawn '${command}'`,
			);
			(error as { code?: string }).code = "EACCES";
			throw error;
		}
	};
	return {
		spawn(command, args, options) {
			check(command, args, options);
			return adapter.spawn(command, args, options);
		},
	};
}

export function wrapFileSystem(
	filesystem: VirtualFileSystem,
	permissions?: Permissions,
): VirtualFileSystem {
	if (!permissions?.fs) return filesystem;
	const check = (path: string, operation: string): void => {
		if (!permissionAllowed(permissions.fs?.({ path, operation }))) {
			throw new Error(`EACCES: blocked ${operation} on '${path}'`);
		}
	};
	return {
		readFile(path) {
			check(path, "readFile");
			return filesystem.readFile(path);
		},
		readTextFile(path) {
			check(path, "readTextFile");
			return filesystem.readTextFile(path);
		},
		readDir(path) {
			check(path, "readDir");
			return filesystem.readDir(path);
		},
		readDirWithTypes(path) {
			check(path, "readDirWithTypes");
			return filesystem.readDirWithTypes(path);
		},
		writeFile(path, content) {
			check(path, "writeFile");
			return filesystem.writeFile(path, content);
		},
		createDir(path) {
			check(path, "createDir");
			return filesystem.createDir(path);
		},
		mkdir(path, options) {
			check(path, "mkdir");
			return filesystem.mkdir(path, options);
		},
		exists(path) {
			check(path, "exists");
			return filesystem.exists(path);
		},
		stat(path) {
			check(path, "stat");
			return filesystem.stat(path);
		},
		removeFile(path) {
			check(path, "removeFile");
			return filesystem.removeFile(path);
		},
		removeDir(path) {
			check(path, "removeDir");
			return filesystem.removeDir(path);
		},
		rename(oldPath, newPath) {
			check(oldPath, "rename");
			check(newPath, "rename");
			return filesystem.rename(oldPath, newPath);
		},
		realpath(path) {
			check(path, "realpath");
			return filesystem.realpath(path);
		},
		symlink(target, linkPath) {
			check(linkPath, "symlink");
			return filesystem.symlink(target, linkPath);
		},
		readlink(path) {
			check(path, "readlink");
			return filesystem.readlink(path);
		},
		lstat(path) {
			check(path, "lstat");
			return filesystem.lstat(path);
		},
		link(oldPath, newPath) {
			check(oldPath, "link");
			check(newPath, "link");
			return filesystem.link(oldPath, newPath);
		},
		chmod(path, mode) {
			check(path, "chmod");
			return filesystem.chmod(path, mode);
		},
		chown(path, uid, gid) {
			check(path, "chown");
			return filesystem.chown(path, uid, gid);
		},
		utimes(path, atime, mtime) {
			check(path, "utimes");
			return filesystem.utimes(path, atime, mtime);
		},
		truncate(path, length) {
			check(path, "truncate");
			return filesystem.truncate(path, length);
		},
		pread(path, offset, length) {
			check(path, "pread");
			return filesystem.pread(path, offset, length);
		},
		pwrite(path, offset, data) {
			check(path, "pwrite");
			return filesystem.pwrite(path, offset, data);
		},
	};
}

export function wrapNetworkAdapter(
	adapter: NetworkAdapter,
	permissions?: Permissions,
): NetworkAdapter {
	if (!permissions?.network) return adapter;
	const check = (request: {
		url?: string;
		host?: string;
		port?: number;
	}): void => {
		if (!permissionAllowed(permissions.network?.(request))) {
			throw new Error(
				`EACCES: blocked network access to '${request.url ?? request.host ?? ""}'`,
			);
		}
	};
	return {
		async fetch(url, options) {
			check({ url });
			return adapter.fetch(url, options);
		},
		async dnsLookup(hostname) {
			check({ host: hostname });
			return adapter.dnsLookup(hostname);
		},
		async httpRequest(url, options) {
			check({ url });
			return adapter.httpRequest(url, options);
		},
	};
}

export async function mkdir(
	filesystem: VirtualFileSystem,
	path: string,
	options?: { recursive?: boolean } | boolean,
): Promise<void> {
	if (typeof options === "boolean") {
		return filesystem.mkdir(path, { recursive: options });
	}
	return filesystem.mkdir(path, options);
}

export async function loadFile(
	path: string,
	filesystem: VirtualFileSystem,
): Promise<string | null> {
	try {
		return await filesystem.readTextFile(path);
	} catch {
		return null;
	}
}

export async function moduleFormat(
	path: string,
	filesystem: VirtualFileSystem,
): Promise<"module" | "commonjs" | "json" | null> {
	if (path.startsWith("node:")) {
		return "module";
	}

	const normalized = normalizePath(path);
	if (normalized.endsWith(".mjs") || normalized.endsWith(".mts")) {
		return "module";
	}
	if (normalized.endsWith(".cjs") || normalized.endsWith(".cts")) {
		return "commonjs";
	}
	if (normalized.endsWith(".json")) {
		return "json";
	}
	if (!normalized.endsWith(".js")) {
		return null;
	}

	const packageType = await nearestPackageJsonType(normalized, filesystem);
	return packageType === "module" ? "module" : "commonjs";
}

export async function resolveModule(
	specifier: string,
	fromPath: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import" = "require",
): Promise<string | null> {
	if (specifier.startsWith("node:")) {
		return specifier;
	}

	const fromDir = await resolveImporterDir(fromPath, filesystem);
	if (specifier.startsWith("file:")) {
		const filePath = fileUrlToPath(specifier);
		return filePath === null ? null : resolvePath(filePath, filesystem, mode);
	}
	if (specifier.startsWith(".") || specifier.startsWith("/")) {
		const base = specifier.startsWith("/")
			? specifier
			: `${fromDir}/${specifier}`;
		return resolvePath(base, filesystem, mode);
	}
	if (specifier.startsWith("#")) {
		return resolvePackageImports(specifier, fromDir, filesystem, mode);
	}

	return (
		(await resolvePackageSelfReference(specifier, fromDir, filesystem, mode)) ??
		resolveNodeModules(specifier, fromDir, filesystem, mode)
	);
}

function fileUrlToPath(specifier: string): string | null {
	let pathname: string;
	if (specifier.startsWith("file://")) {
		pathname = specifier.slice("file://".length);
	} else {
		pathname = specifier.slice("file:".length);
	}
	const terminator = pathname.search(/[?#]/);
	if (terminator >= 0) {
		pathname = pathname.slice(0, terminator);
	}
	if (!pathname.startsWith("/")) {
		const slashIndex = pathname.indexOf("/");
		if (slashIndex < 0) {
			return null;
		}
		const host = pathname.slice(0, slashIndex);
		if (host !== "" && host !== "localhost") {
			return null;
		}
		pathname = pathname.slice(slashIndex);
	}
	try {
		return normalizePath(decodeURIComponent(pathname));
	} catch {
		return null;
	}
}

async function resolveImporterDir(
	fromPath: string,
	filesystem: VirtualFileSystem,
): Promise<string> {
	let fromDir = normalizePath(fromPath);
	try {
		const stat = await filesystem.stat(fromDir);
		if (!stat.isDirectory) {
			return dirname(fromDir);
		}
		return await realpathOrSelf(fromDir, filesystem);
	} catch {
		const basename = fromDir.split("/").at(-1) ?? "";
		if (basename.includes(".")) {
			fromDir = dirname(fromDir);
		}
		return fromDir;
	}
}

async function realpathOrSelf(
	path: string,
	filesystem: VirtualFileSystem,
): Promise<string> {
	try {
		return normalizePath(await filesystem.realpath(path));
	} catch {
		return normalizePath(path);
	}
}

async function resolveNodeModules(
	specifier: string,
	fromDir: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	const parsed = parsePackageSpecifier(specifier);
	if (!parsed) {
		return null;
	}

	for (const dir of ancestorDirs(fromDir)) {
		if (dir === "/node_modules" || dir.endsWith("/node_modules")) {
			continue;
		}
		const packageDir = normalizePath(`${dir}/node_modules/${parsed.name}`);
		const resolved = await resolvePackageEntry(
			packageDir,
			parsed.subpath,
			filesystem,
			mode,
		);
		if (resolved) {
			return resolved;
		}
	}

	for (const root of ["/root/node_modules", "/node_modules"]) {
		const resolved = await resolvePackageEntry(
			normalizePath(`${root}/${parsed.name}`),
			parsed.subpath,
			filesystem,
			mode,
		);
		if (resolved) {
			return resolved;
		}
	}

	return null;
}

async function resolvePackageImports(
	specifier: string,
	fromDir: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	let dir = normalizePath(fromDir);
	while (true) {
		const packageJson = await readPackageJson(dir, filesystem);
		if (packageJson && Object.hasOwn(packageJson, "imports")) {
			const target = resolveImportsTarget(packageJson.imports, specifier, mode);
			if (!target) {
				return null;
			}
			const targetPath = target.startsWith("/") ? target : `${dir}/${target}`;
			return resolvePath(targetPath, filesystem, mode);
		}
		if (dir === "/") {
			break;
		}
		dir = dirname(dir);
	}
	return null;
}

async function resolvePackageSelfReference(
	specifier: string,
	fromDir: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	const parsed = parsePackageSpecifier(specifier);
	if (!parsed) {
		return null;
	}

	for (const dir of ancestorDirs(fromDir)) {
		const packageJson = await readPackageJson(dir, filesystem);
		if (packageJson && packageJson.name === parsed.name) {
			return resolvePackageEntry(dir, parsed.subpath, filesystem, mode);
		}
	}

	return null;
}

function parsePackageSpecifier(
	specifier: string,
): { name: string; subpath: string } | null {
	const parts = specifier.split("/").filter(Boolean);
	if (parts.length === 0) {
		return null;
	}
	if (parts[0]?.startsWith("@")) {
		if (parts.length < 2) {
			return null;
		}
		return {
			name: `${parts[0]}/${parts[1]}`,
			subpath: parts.slice(2).join("/"),
		};
	}
	return {
		name: parts[0] ?? "",
		subpath: parts.slice(1).join("/"),
	};
}

function ancestorDirs(fromDir: string): string[] {
	const dirs: string[] = [];
	let current = normalizePath(fromDir);
	while (true) {
		dirs.push(current);
		if (current === "/") {
			break;
		}
		current = dirname(current);
	}
	return dirs;
}

async function resolvePackageEntry(
	packageDir: string,
	subpath: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	const packageJson = await readPackageJson(packageDir, filesystem);
	if (packageJson && Object.hasOwn(packageJson, "exports")) {
		const exportsSubpath = subpath ? `./${subpath}` : ".";
		const exportsTarget = resolveExportsTarget(
			packageJson.exports,
			exportsSubpath,
			mode,
		);
		if (!exportsTarget) {
			return null;
		}
		const targetPath = normalizePath(`${packageDir}/${exportsTarget}`);
		return (await resolvePath(targetPath, filesystem, mode)) ?? targetPath;
	}

	if (subpath) {
		return resolvePath(`${packageDir}/${subpath}`, filesystem, mode);
	}

	if (typeof packageJson?.main === "string" && packageJson.main.length > 0) {
		const mainResolved = await resolvePath(
			`${packageDir}/${packageJson.main}`,
			filesystem,
			mode,
		);
		if (mainResolved) {
			return mainResolved;
		}
	}

	return resolvePath(`${packageDir}/index`, filesystem, mode);
}

async function resolvePath(
	basePath: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	return (
		(await resolveAsFile(basePath, filesystem)) ??
		resolveAsDirectory(basePath, filesystem, mode)
	);
}

async function resolveAsFile(
	basePath: string,
	filesystem: VirtualFileSystem,
): Promise<string | null> {
	const normalized = normalizePath(basePath);
	const candidates = [
		normalized,
		`${normalized}.js`,
		`${normalized}.json`,
		`${normalized}.mjs`,
		`${normalized}.cjs`,
	];
	for (const candidate of candidates) {
		try {
			const stat = await filesystem.stat(candidate);
			if (!stat.isDirectory) {
				return realpathOrSelf(candidate, filesystem);
			}
		} catch {
			// Try the next candidate.
		}
	}
	return null;
}

async function resolveAsDirectory(
	basePath: string,
	filesystem: VirtualFileSystem,
	mode: "require" | "import",
): Promise<string | null> {
	const dir = normalizePath(basePath);
	try {
		const stat = await filesystem.stat(dir);
		if (!stat.isDirectory) {
			return null;
		}
	} catch {
		return null;
	}

	const packageJson = await readPackageJson(dir, filesystem);
	if (packageJson && Object.hasOwn(packageJson, "exports")) {
		const exportsTarget = resolveExportsTarget(packageJson.exports, ".", mode);
		if (exportsTarget) {
			const resolved = await resolvePath(
				`${dir}/${exportsTarget}`,
				filesystem,
				mode,
			);
			if (resolved) {
				return resolved;
			}
		}
	}
	if (typeof packageJson?.main === "string" && packageJson.main.length > 0) {
		const mainResolved = await resolvePath(
			`${dir}/${packageJson.main}`,
			filesystem,
			mode,
		);
		if (mainResolved) {
			return mainResolved;
		}
	}
	return resolveAsFile(`${dir}/index`, filesystem);
}

async function readPackageJson(
	packageDir: string,
	filesystem: VirtualFileSystem,
): Promise<Record<string, unknown> | null> {
	try {
		const source = await filesystem.readTextFile(
			normalizePath(`${packageDir}/package.json`),
		);
		const parsed = JSON.parse(source);
		return parsed && typeof parsed === "object"
			? (parsed as Record<string, unknown>)
			: null;
	} catch {
		return null;
	}
}

async function nearestPackageJsonType(
	filePath: string,
	filesystem: VirtualFileSystem,
): Promise<string | null> {
	for (const dir of ancestorDirs(dirname(filePath))) {
		const packageJson = await readPackageJson(dir, filesystem);
		if (packageJson && typeof packageJson.type === "string") {
			return packageJson.type;
		}
	}
	return null;
}

function resolveExportsTarget(
	exportsField: unknown,
	subpath: string,
	mode: "require" | "import",
): string | null {
	const resolved = resolveExportsValue(exportsField, subpath, mode);
	if (typeof resolved !== "string" || !resolved.startsWith("./")) {
		return null;
	}
	return resolved.slice(2);
}

function resolveExportsValue(
	value: unknown,
	subpath: string,
	mode: "require" | "import",
): string | null {
	if (typeof value === "string") {
		return subpath === "." ? value : null;
	}
	if (Array.isArray(value)) {
		for (const item of value) {
			const resolved = resolveExportsValue(item, subpath, mode);
			if (resolved) {
				return resolved;
			}
		}
		return null;
	}
	if (!value || typeof value !== "object") {
		return null;
	}

	const entries = Object.entries(value as Record<string, unknown>);
	const hasSubpathKeys = entries.some(
		([key]) => key === "." || key.startsWith("./"),
	);
	if (hasSubpathKeys) {
		const exact = (value as Record<string, unknown>)[subpath];
		if (exact !== undefined) {
			const resolved = resolveExportsValue(exact, ".", mode);
			if (resolved) {
				return resolved;
			}
		}
		for (const [key, target] of entries) {
			if (!key.includes("*")) {
				continue;
			}
			const [prefix, suffix] = key.split("*", 2);
			if (!prefix || suffix === undefined) {
				continue;
			}
			if (subpath.startsWith(prefix) && subpath.endsWith(suffix)) {
				const wildcard = subpath.slice(
					prefix.length,
					subpath.length - suffix.length,
				);
				const resolved = resolveExportsValue(target, ".", mode);
				if (resolved) {
					return resolved.replaceAll("*", wildcard);
				}
			}
		}
		return null;
	}

	const conditions =
		mode === "import"
			? ["import", "node", "module", "default", "require"]
			: ["require", "node", "default", "import", "module"];
	for (const condition of conditions) {
		const target = (value as Record<string, unknown>)[condition];
		if (target === undefined) {
			continue;
		}
		const resolved = resolveExportsValue(target, subpath, mode);
		if (resolved) {
			return resolved;
		}
	}
	return null;
}

function resolveImportsTarget(
	importsField: unknown,
	specifier: string,
	mode: "require" | "import",
): string | null {
	if (typeof importsField === "string") {
		return importsField;
	}
	if (Array.isArray(importsField)) {
		for (const item of importsField) {
			const resolved = resolveImportsTarget(item, specifier, mode);
			if (resolved) {
				return resolved;
			}
		}
		return null;
	}
	if (!importsField || typeof importsField !== "object") {
		return null;
	}
	const record = importsField as Record<string, unknown>;
	if (Object.hasOwn(record, specifier)) {
		return resolveExportsValue(record[specifier], ".", mode);
	}
	for (const [key, target] of Object.entries(record)) {
		const wildcardIndex = key.indexOf("*");
		if (wildcardIndex < 0) {
			continue;
		}
		const prefix = key.slice(0, wildcardIndex);
		const suffix = key.slice(wildcardIndex + 1);
		if (specifier.startsWith(prefix) && specifier.endsWith(suffix)) {
			const wildcard = specifier.slice(
				prefix.length,
				specifier.length - suffix.length,
			);
			const resolved = resolveExportsValue(target, ".", mode);
			if (resolved) {
				return resolved.replaceAll("*", wildcard);
			}
		}
	}
	return null;
}

export function isESM(code: string, filePath?: string): boolean {
	if (filePath?.endsWith(".mjs")) return true;
	// `.cjs` is explicitly CommonJS — never apply the ESM import transform (it would
	// trip on a large bundle whose only "import" is dynamic `import()`, which is valid
	// in CJS and is NOT an ESM module marker).
	if (filePath?.endsWith(".cjs")) return false;
	return /\b(import|export)\b/.test(code);
}

export function transformDynamicImport(code: string): string {
	// Route dynamic `import(...)` through the executor's module loader
	// (`__dynamicImport`, which resolves via require) instead of the browser's native
	// dynamic import — the latter cannot fetch bare/`node:` specifiers (e.g.
	// `import("node:fs")`) that a guest program's lazy loads use. Only rewrites the
	// call form `import(` preceded by a non-identifier char; leaves `import.meta` and
	// static `import ` statements untouched.
	return code.replace(/(^|[^.\w$])import(\s*\()/g, "$1__dynamicImport$2");
}

export const POLYFILL_CODE_MAP: Record<string, string> = {
	fs: "module.exports = globalThis._fsModule;",
	"node:fs": "module.exports = globalThis._fsModule;",
	"fs/promises":
		"module.exports = globalThis._fsModule.promises || globalThis._fsModule;",
	"node:fs/promises":
		"module.exports = globalThis._fsModule.promises || globalThis._fsModule;",
	util: BROWSER_UTIL_POLYFILL_CODE,
	"node:util": "module.exports = require('util');",
	"util/types": "module.exports = require('util').types;",
	"node:util/types": "module.exports = require('util/types');",
	buffer: BROWSER_BUFFER_POLYFILL_CODE,
	"node:buffer": "module.exports = require('buffer');",
	path: BROWSER_PATH_POLYFILL_CODE,
	"node:path": "module.exports = require('path');",
	console: "module.exports = globalThis.console;",
	"node:console": "module.exports = require('console');",
	process: "module.exports = globalThis.process;",
	"node:process": "module.exports = globalThis.process;",
	// node:module — createRequire returns the guest's kernel-backed require so guest
	// programs (e.g. the pi ACP adapter) can build a require from import.meta.url.
	module: `
		const createRequire = () => globalThis.require;
		const Module = { createRequire };
		module.exports = { createRequire, Module, builtinModules: [] };
		module.exports.default = module.exports;
	`,
	"node:module": "module.exports = require('module');",
	// node:stream — a minimal but functional stream set. The ACP connection itself
	// uses WHATWG Readable/WritableStream (worker globals); guest programs use these
	// node streams for buffering (e.g. pi's bufferedStdin PassThrough). Readable.toWeb
	// / Writable.toWeb bridge to the WHATWG streams the ACP codec consumes.
	stream: `
		class EventEmitterLike {
			constructor() { this._listeners = Object.create(null); }
			on(event, fn) { (this._listeners[event] = this._listeners[event] || []).push(fn); return this; }
			addListener(event, fn) { return this.on(event, fn); }
			once(event, fn) { const w = (...a) => { this.off(event, w); fn(...a); }; w._origin = fn; return this.on(event, w); }
			off(event, fn) { if (this._listeners[event]) this._listeners[event] = this._listeners[event].filter((x) => x !== fn && x._origin !== fn); return this; }
			removeListener(event, fn) { return this.off(event, fn); }
			removeAllListeners(event) { if (event) delete this._listeners[event]; else this._listeners = Object.create(null); return this; }
			emit(event, ...args) { const ls = (this._listeners[event] || []).slice(); for (const fn of ls) fn(...args); return ls.length > 0; }
			listenerCount(event) { return (this._listeners[event] || []).length; }
		}
		class Readable extends EventEmitterLike {
			constructor(options) { super(); this.readable = true; this._readableOptions = options || {}; if (this._readableOptions.read) this._read = this._readableOptions.read; }
			resume() { this.emit("resume"); return this; }
			pause() { this.paused = true; return this; }
			setEncoding() { return this; }
			read() { return null; }
			push(chunk) { if (chunk == null) this.emit("end"); else this.emit("data", chunk); return true; }
			pipe(dest) { this.on("data", (c) => dest.write && dest.write(c)); this.on("end", () => dest.end && dest.end()); return dest; }
			destroy() { this.emit("close"); return this; }
		}
		Readable.toWeb = (stream) => new ReadableStream({ start(controller) {
			stream.on("data", (chunk) => controller.enqueue(chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk)));
			stream.on("end", () => { try { controller.close(); } catch (e) {} });
			stream.on("error", (err) => controller.error(err));
		} });
		class Writable extends EventEmitterLike {
			constructor(options) { super(); this.writable = true; this._writableOptions = options || {}; if (this._writableOptions.write) this._writeImpl = this._writableOptions.write; }
			write(chunk, encoding, cb) { if (typeof encoding === "function") { cb = encoding; encoding = undefined; } if (this._writeImpl) this._writeImpl(chunk, encoding, cb || (() => {})); else if (cb) cb(); this.emit("data", chunk); return true; }
			end(chunk, encoding, cb) { const done = typeof chunk === "function" ? chunk : typeof encoding === "function" ? encoding : cb; if (chunk != null && typeof chunk !== "function") this.write(chunk); this.emit("finish"); this.emit("end"); if (done) done(); }
			destroy() { this.emit("close"); return this; }
		}
		Writable.toWeb = (stream) => new WritableStream({ write(chunk) { return new Promise((resolve) => stream.write(chunk, undefined, () => resolve())); }, close() { stream.end && stream.end(); } });
		class Duplex extends Readable { constructor(options) { super(options); this.writable = true; if (options && options.write) this._writeImpl = options.write; } write(chunk, encoding, cb) { if (typeof encoding === "function") { cb = encoding; } if (this._writeImpl) this._writeImpl(chunk, encoding, cb || (() => {})); else if (cb) cb(); return true; } end(chunk) { if (chunk != null) this.write(chunk); this.emit("finish"); this.emit("end"); } }
		class Transform extends Duplex {}
		class PassThrough extends Transform { write(chunk, encoding, cb) { if (typeof encoding === "function") { cb = encoding; } this.emit("data", chunk); if (cb) cb(); return true; } end(chunk) { if (chunk != null) this.emit("data", chunk); this.emit("end"); this.emit("finish"); } }
		function finished(stream, optsOrCb, maybeCb) {
			const cb = typeof optsOrCb === "function" ? optsOrCb : maybeCb;
			if (stream && stream.on) { let done = false; const fire = (e) => { if (done) return; done = true; if (cb) cb(e || null); }; stream.on("end", () => fire()); stream.on("finish", () => fire()); stream.on("close", () => fire()); stream.on("error", (e) => fire(e)); }
			return () => {};
		}
		function pipeline(...args) {
			const cb = typeof args[args.length - 1] === "function" ? args.pop() : null;
			const streams = args.flat();
			for (let i = 0; i < streams.length - 1; i++) { if (streams[i] && streams[i].pipe) streams[i].pipe(streams[i + 1]); }
			const last = streams[streams.length - 1];
			if (last && last.on) { last.on("finish", () => cb && cb(null)); last.on("end", () => cb && cb(null)); last.on("error", (e) => cb && cb(e)); }
			return last;
		}
		const Stream = EventEmitterLike;
		Stream.Readable = Readable; Stream.Writable = Writable; Stream.Duplex = Duplex; Stream.Transform = Transform; Stream.PassThrough = PassThrough;
		module.exports = { Stream, Readable, Writable, Duplex, Transform, PassThrough, finished, pipeline };
		module.exports.promises = { finished: (s) => new Promise((res, rej) => finished(s, (e) => (e ? rej(e) : res()))), pipeline: (...a) => new Promise((res, rej) => pipeline(...a, (e) => (e ? rej(e) : res()))) };
		module.exports.default = module.exports;
	`,
	"node:stream": "module.exports = require('stream');",
	"stream/promises": "module.exports = require('stream').promises;",
	"node:stream/promises": "module.exports = require('stream').promises;",
	"stream/web":
		"module.exports = { ReadableStream: globalThis.ReadableStream, WritableStream: globalThis.WritableStream, TransformStream: globalThis.TransformStream };",
	"node:stream/web": "module.exports = require('stream/web');",
	// node:constants — fs/os constant values guest programs reference (open flags, etc.).
	constants: `
		module.exports = {
			O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 64, O_EXCL: 128, O_NOCTTY: 256,
			O_TRUNC: 512, O_APPEND: 1024, O_DIRECTORY: 65536, O_NOFOLLOW: 131072, O_SYNC: 1052672,
			O_NONBLOCK: 2048, S_IFMT: 61440, S_IFREG: 32768, S_IFDIR: 16384, S_IFCHR: 8192,
			S_IFLNK: 40960, S_IFIFO: 4096, S_IFSOCK: 49152, F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1,
			COPYFILE_EXCL: 1, SIGINT: 2, SIGTERM: 15, SIGKILL: 9, SIGHUP: 1,
		};
		module.exports.default = module.exports;
	`,
	"node:constants": "module.exports = require('constants');",
	// node:events — EventEmitter (a complete-enough implementation for guest libraries).
	events: `
		class EventEmitter {
			constructor() { this._events = Object.create(null); this._max = 10; }
			setMaxListeners(n) { this._max = n; return this; }
			getMaxListeners() { return this._max; }
			on(type, fn) { (this._events[type] = this._events[type] || []).push(fn); this.emit("newListener", type, fn); return this; }
			addListener(type, fn) { return this.on(type, fn); }
			prependListener(type, fn) { (this._events[type] = this._events[type] || []).unshift(fn); return this; }
			once(type, fn) { const w = (...a) => { this.off(type, w); fn(...a); }; w.listener = fn; return this.on(type, w); }
			prependOnceListener(type, fn) { const w = (...a) => { this.off(type, w); fn(...a); }; w.listener = fn; return this.prependListener(type, w); }
			off(type, fn) { const l = this._events[type]; if (l) { this._events[type] = l.filter((x) => x !== fn && x.listener !== fn); if (this._events[type].length === 0) delete this._events[type]; } return this; }
			removeListener(type, fn) { return this.off(type, fn); }
			removeAllListeners(type) { if (type) delete this._events[type]; else this._events = Object.create(null); return this; }
			emit(type, ...args) { const l = this._events[type]; if (!l || l.length === 0) { if (type === "error") throw args[0] instanceof Error ? args[0] : new Error("Unhandled error"); return false; } for (const fn of l.slice()) fn.apply(this, args); return true; }
			listeners(type) { return (this._events[type] || []).slice(); }
			rawListeners(type) { return (this._events[type] || []).slice(); }
			listenerCount(type) { return (this._events[type] || []).length; }
			eventNames() { return Object.keys(this._events); }
		}
		EventEmitter.EventEmitter = EventEmitter;
		EventEmitter.once = (emitter, name) => new Promise((resolve, reject) => {
			const ok = (...a) => { emitter.off("error", err); resolve(a); };
			const err = (e) => { emitter.off(name, ok); reject(e); };
			emitter.once(name, ok); emitter.once("error", err);
		});
		EventEmitter.defaultMaxListeners = 10;
		module.exports = EventEmitter;
		module.exports.default = EventEmitter;
	`,
	"node:events": "module.exports = require('events');",
	// node:assert — the common assertion surface.
	assert: `
		function AssertionError(message) { const e = new Error(message); e.name = "AssertionError"; return e; }
		function assert(value, message) { if (!value) throw AssertionError(message || "assertion failed"); }
		assert.ok = assert;
		assert.equal = (a, b, m) => { if (a != b) throw AssertionError(m || (a + " != " + b)); };
		assert.strictEqual = (a, b, m) => { if (a !== b) throw AssertionError(m || (a + " !== " + b)); };
		assert.notEqual = (a, b, m) => { if (a == b) throw AssertionError(m); };
		assert.notStrictEqual = (a, b, m) => { if (a === b) throw AssertionError(m); };
		assert.deepEqual = (a, b, m) => { if (JSON.stringify(a) !== JSON.stringify(b)) throw AssertionError(m); };
		assert.deepStrictEqual = assert.deepEqual;
		assert.fail = (m) => { throw AssertionError(m || "failed"); };
		assert.throws = (fn, m) => { try { fn(); } catch (e) { return; } throw AssertionError(m || "missing expected exception"); };
		assert.AssertionError = AssertionError;
		module.exports = assert;
		module.exports.default = assert;
	`,
	"node:assert": "module.exports = require('assert');",
	// node:url — WHATWG URL globals + the legacy parse/format surface.
	url: `
		module.exports = {
			URL: globalThis.URL,
			URLSearchParams: globalThis.URLSearchParams,
			parse(input) { try { const u = new URL(input); return { href: u.href, protocol: u.protocol, host: u.host, hostname: u.hostname, port: u.port, pathname: u.pathname, search: u.search, hash: u.hash, query: u.search.replace(/^\\?/, ""), path: u.pathname + u.search }; } catch (e) { return { href: input, pathname: input }; } },
			format(u) { if (typeof u === "string") return u; const proto = u.protocol ? (u.protocol.endsWith(":") ? u.protocol : u.protocol + ":") : ""; return proto + "//" + (u.host || u.hostname || "") + (u.pathname || "") + (u.search || (u.query ? "?" + u.query : "")) + (u.hash || ""); },
			resolve(from, to) { try { return new URL(to, from).href; } catch (e) { return to; } },
			fileURLToPath(u) { const s = typeof u === "string" ? u : u.href; return s.replace(/^file:\\/\\//, ""); },
			pathToFileURL(p) { return new URL("file://" + (p.startsWith("/") ? p : "/" + p)); },
			domainToASCII: (d) => d,
			domainToUnicode: (d) => d,
		};
		module.exports.default = module.exports;
	`,
	"node:url": "module.exports = require('url');",
	// node:string_decoder — UTF-8 incremental decoder (TextDecoder-backed).
	string_decoder: `
		class StringDecoder {
			constructor(encoding) { this.encoding = encoding || "utf8"; this._decoder = new TextDecoder(this.encoding === "utf8" ? "utf-8" : this.encoding); }
			write(buf) { const bytes = buf instanceof Uint8Array ? buf : new Uint8Array(buf); return this._decoder.decode(bytes, { stream: true }); }
			end(buf) { const head = buf ? this.write(buf) : ""; return head + this._decoder.decode(); }
		}
		module.exports = { StringDecoder };
		module.exports.default = module.exports;
	`,
	"node:string_decoder": "module.exports = require('string_decoder');",
	// node:querystring — legacy query parsing/serialization.
	querystring: `
		module.exports = {
			parse(str) { const out = Object.create(null); if (!str) return out; for (const pair of String(str).split("&")) { if (!pair) continue; const i = pair.indexOf("="); const k = decodeURIComponent(i < 0 ? pair : pair.slice(0, i)); const v = i < 0 ? "" : decodeURIComponent(pair.slice(i + 1)); if (k in out) { if (Array.isArray(out[k])) out[k].push(v); else out[k] = [out[k], v]; } else out[k] = v; } return out; },
			stringify(obj) { if (!obj) return ""; const parts = []; for (const k of Object.keys(obj)) { const v = obj[k]; if (Array.isArray(v)) for (const item of v) parts.push(encodeURIComponent(k) + "=" + encodeURIComponent(item)); else parts.push(encodeURIComponent(k) + "=" + encodeURIComponent(v)); } return parts.join("&"); },
			escape: encodeURIComponent, unescape: decodeURIComponent,
		};
		module.exports.default = module.exports;
	`,
	"node:querystring": "module.exports = require('querystring');",
	// node:tty — reflects ExecOptions.stdioPty for stdio fds.
	tty: `
		const ttyState = () => globalThis.__agentOSTtyState;
		class ReadStream {
			constructor(fd) { this.fd = fd; this.isTTY = !!ttyState()?.isatty?.(fd); }
			setRawMode(mode) { if (this.fd === 0 && globalThis.process?.stdin?.setRawMode) globalThis.process.stdin.setRawMode(mode); return this; }
		}
		class WriteStream {
			constructor(fd) { this.fd = fd; this.isTTY = !!ttyState()?.isatty?.(fd); }
			get columns() { return ttyState()?.columns?.() ?? 80; }
			get rows() { return ttyState()?.rows?.() ?? 24; }
		}
		module.exports = {
			isatty: (fd) => !!ttyState()?.isatty?.(fd),
			ReadStream,
			WriteStream,
		};
		module.exports.default = module.exports;
	`,
	"node:tty": "module.exports = require('tty');",
	// node:readline — stub interface (in ACP mode stdin is the protocol, not a REPL).
	readline: `
		module.exports = {
			createInterface: () => { const rl = { on: () => rl, once: () => rl, off: () => rl, removeListener: () => rl, removeAllListeners: () => rl, emit: () => false, close: () => {}, question: (q, cb) => { if (typeof cb === "function") cb(""); }, prompt: () => {}, write: () => {}, pause: () => rl, resume: () => rl, setPrompt: () => {}, [Symbol.asyncIterator]: async function* () {} }; return rl; },
			clearLine: () => true, clearScreenDown: () => true, cursorTo: () => true, moveCursor: () => true, emitKeypressEvents: () => {},
		};
		module.exports.default = module.exports;
	`,
	"node:readline": "module.exports = require('readline');",
	"readline/promises": "module.exports = require('readline');",
	"node:readline/promises": "module.exports = require('readline');",
	// node:timers — the timer globals.
	timers: `
		module.exports = { setTimeout: globalThis.setTimeout.bind(globalThis), clearTimeout: globalThis.clearTimeout.bind(globalThis), setInterval: globalThis.setInterval.bind(globalThis), clearInterval: globalThis.clearInterval.bind(globalThis), setImmediate: globalThis.setImmediate, clearImmediate: globalThis.clearImmediate };
		module.exports.default = module.exports;
	`,
	"node:timers": "module.exports = require('timers');",
	"timers/promises": `
		module.exports = { setTimeout: (ms, value) => new Promise((r) => globalThis.setTimeout(() => r(value), ms)), setImmediate: (value) => Promise.resolve(value), setInterval: async function* () {} };
		module.exports.default = module.exports;
	`,
	"node:timers/promises": "module.exports = require('timers/promises');",
	// node:diagnostics_channel / node:inspector — no-op observability stubs.
	diagnostics_channel: `
		module.exports = { channel: () => ({ hasSubscribers: false, publish() {}, subscribe() {}, unsubscribe() {} }), hasSubscribers: () => false, subscribe() {}, unsubscribe() {} };
		module.exports.default = module.exports;
	`,
	"node:diagnostics_channel":
		"module.exports = require('diagnostics_channel');",
	inspector: `module.exports = { open() {}, close() {}, url: () => undefined, Session: class {} }; module.exports.default = module.exports;`,
	"node:inspector": "module.exports = require('inspector');",
	// node:v8 — heap stats + structured serialize (JSON fallback) guest libs may probe.
	v8: `
		module.exports = {
			serialize: (v) => new TextEncoder().encode(JSON.stringify(v)),
			deserialize: (b) => JSON.parse(new TextDecoder().decode(b)),
			getHeapStatistics: () => ({ total_heap_size: 0, used_heap_size: 0, heap_size_limit: 0 }),
			getHeapSpaceStatistics: () => [],
			setFlagsFromString: () => {},
		};
		module.exports.default = module.exports;
	`,
	"node:v8": "module.exports = require('v8');",
	// node:async_hooks — a working single-threaded AsyncLocalStorage (synchronous store
	// stack; context propagation across awaits is best-effort) + no-op AsyncResource.
	async_hooks: `
		class AsyncLocalStorage {
			constructor() { this._stack = []; }
			run(store, fn, ...args) { this._stack.push(store); try { return fn(...args); } finally { this._stack.pop(); } }
			getStore() { return this._stack.length ? this._stack[this._stack.length - 1] : undefined; }
			enterWith(store) { this._stack.push(store); }
			exit(fn, ...args) { const saved = this._stack; this._stack = []; try { return fn(...args); } finally { this._stack = saved; } }
			disable() { this._stack = []; }
		}
		class AsyncResource { constructor() {} runInAsyncScope(fn, thisArg, ...args) { return fn.apply(thisArg, args); } bind(fn) { return fn; } emitDestroy() { return this; } }
		module.exports = { AsyncLocalStorage, AsyncResource, createHook: () => ({ enable() {}, disable() {} }), executionAsyncId: () => 0, triggerAsyncId: () => 0 };
		module.exports.default = module.exports;
	`,
	"node:async_hooks": "module.exports = require('async_hooks');",
	// node:perf_hooks — the performance global + a no-op observer.
	perf_hooks: `
		module.exports = {
			performance: globalThis.performance,
			PerformanceObserver: class { constructor() {} observe() {} disconnect() {} },
			monitorEventLoopDelay: () => ({ enable() {}, disable() {}, reset() {} }),
		};
		module.exports.default = module.exports;
	`,
	"node:perf_hooks": "module.exports = require('perf_hooks');",
	// node:zlib — present but unsupported; throws only if actually used (often imported,
	// not exercised, on the guest happy path).
	zlib: `
		const unsupported = () => { throw new Error("zlib is not supported in the browser runtime"); };
		module.exports = { gzip: unsupported, gunzip: unsupported, gzipSync: unsupported, gunzipSync: unsupported, deflate: unsupported, inflate: unsupported, deflateSync: unsupported, inflateSync: unsupported, brotliCompressSync: unsupported, brotliDecompressSync: unsupported, createGzip: unsupported, createGunzip: unsupported, constants: {} };
		module.exports.default = module.exports;
	`,
	"node:zlib": "module.exports = require('zlib');",
	// node:http / node:https — guest HTTP belongs to global fetch (kernel-brokered);
	// the legacy module surface is a stub that errors only if actually used.
	http: `
		const unsupported = () => { throw new Error("node:http is not supported; use global fetch"); };
		module.exports = { request: unsupported, get: unsupported, createServer: unsupported, Agent: class {}, globalAgent: {}, STATUS_CODES: {}, METHODS: [] };
		module.exports.default = module.exports;
	`,
	"node:http": "module.exports = require('http');",
	https: `module.exports = require('http');`,
	"node:https": "module.exports = require('http');",
	// node:net — stub (kernel sockets are reached via the converged net bridge, not this).
	net: `
		const unsupported = () => { throw new Error("node:net is not supported in this runtime"); };
		module.exports = { connect: unsupported, createConnection: unsupported, createServer: unsupported, Socket: class {}, isIP: () => 0, isIPv4: () => false, isIPv6: () => false };
		module.exports.default = module.exports;
	`,
	"node:net": "module.exports = require('net');",
	// node:vm — minimal: run code in the guest global scope.
	vm: `
		module.exports = {
			runInThisContext: (code) => (0, eval)(code),
			runInNewContext: (code) => (0, eval)(code),
			createContext: (o) => o || {},
			Script: class { constructor(code) { this.code = code; } runInThisContext() { return (0, eval)(this.code); } runInNewContext() { return (0, eval)(this.code); } },
		};
		module.exports.default = module.exports;
	`,
	"node:vm": "module.exports = require('vm');",
	// node:worker_threads — single-threaded: main thread, no spawning.
	worker_threads: `
		module.exports = { isMainThread: true, threadId: 0, parentPort: null, workerData: null, Worker: class { constructor() { throw new Error("worker_threads is not supported in this runtime"); } }, MessageChannel: class {}, MessagePort: class {} };
		module.exports.default = module.exports;
	`,
	"node:worker_threads": "module.exports = require('worker_threads');",
	child_process: `
		const callSync = (ref, ...args) => {
			if (typeof ref === "function") return ref(...args);
			if (ref && typeof ref.applySync === "function") return ref.applySync(undefined, args);
			if (ref && typeof ref.applySyncPromise === "function") return ref.applySyncPromise(undefined, args);
			throw new Error("child_process bridge is not configured");
		};
		const encodeBytes = globalThis.__agentOSEncoding.encodeBytesPayload;
		const decodeBytes = globalThis.__agentOSEncoding.decodeBytesPayload;
		const text = (bytes) => new TextDecoder().decode(bytes);
		const bufferLike = (value) => {
			const bytes = decodeBytes(value);
			bytes.toString = () => text(bytes);
			return bytes;
		};
		class Emitter {
			constructor() {
				this._listeners = new Map();
			}
			on(event, listener) {
				const listeners = this._listeners.get(event) || [];
				listeners.push(listener);
				this._listeners.set(event, listeners);
				return this;
			}
			once(event, listener) {
				const wrapped = (...args) => {
					this.off(event, wrapped);
					listener(...args);
				};
				return this.on(event, wrapped);
			}
			off(event, listener) {
				const listeners = this._listeners.get(event) || [];
				this._listeners.set(event, listeners.filter((entry) => entry !== listener));
				return this;
			}
			removeListener(event, listener) {
				return this.off(event, listener);
			}
			emit(event, ...args) {
				const listeners = this._listeners.get(event) || [];
				for (const listener of [...listeners]) listener(...args);
				return listeners.length > 0;
			}
		}
		class ChildProcess extends Emitter {
			constructor(sessionId) {
				super();
				this.pid = Number(sessionId) || -1;
				this.exitCode = null;
				this.signalCode = null;
				this.killed = false;
				this.stdout = new Emitter();
				this.stderr = new Emitter();
				this.stdin = {
					write: (data) => {
						callSync(globalThis._childProcessStdinWrite, sessionId, typeof data === "string" ? new TextEncoder().encode(data) : data);
						return true;
					},
					end: (data) => {
						if (data != null) this.stdin.write(data);
						callSync(globalThis._childProcessStdinClose, sessionId);
					},
				};
			}
		}
		const normalizeArgs = (args, options) => {
			if (Array.isArray(args)) return { args, options: options || {} };
			return { args: [], options: args || {} };
		};
		const signalNumbers = ${JSON.stringify(PROCESS_SIGNAL_NUMBERS)};
		const normalizeSignal = (signal) => {
			if (signal === undefined || signal === null) return 15;
			if (typeof signal === "number" && Number.isFinite(signal)) {
				const numeric = Math.trunc(signal);
				if (numeric >= 0 && numeric <= 31) return numeric;
				throw unknownSignalError(signal);
			}
			const raw = String(signal).trim();
			if (/^[+-]?\\d+$/.test(raw)) {
				const numeric = Number.parseInt(raw, 10);
				if (numeric >= 0 && numeric <= 31) return numeric;
				throw unknownSignalError(signal);
			}
			const upper = raw.toUpperCase();
			const signalName = upper.startsWith("SIG") ? upper : "SIG" + upper;
			const numeric = signalNumbers[signalName];
			if (numeric !== undefined) return numeric;
			throw unknownSignalError(signal);
		};
		const unknownSignalError = (signal) => {
			const error = new TypeError("Unknown signal: " + String(signal));
			error.code = "ERR_UNKNOWN_SIGNAL";
			return error;
		};
		function spawn(command, argsOrOptions, maybeOptions) {
			const { args, options } = normalizeArgs(argsOrOptions, maybeOptions);
			let sessionId;
			try {
				sessionId = callSync(
					globalThis._childProcessSpawnStart,
					{
						command: String(command),
						args: args.map(String),
						options: {
							cwd: options.cwd || (globalThis.process && globalThis.process.cwd ? globalThis.process.cwd() : "/"),
							env: options.env,
						},
					},
				);
			} catch (error) {
				const child = new ChildProcess(-1);
				queueMicrotask(() => child.emit("error", error));
				return child;
			}
			const child = new ChildProcess(sessionId);
			child.kill = (signal) => {
				callSync(globalThis._childProcessKill, sessionId, normalizeSignal(signal));
				child.killed = true;
				return true;
			};
			const poll = () => {
				const event = callSync(globalThis._childProcessPoll, sessionId, 0);
				if (!event) {
					setTimeout(poll, 0);
					return;
				}
				if (event.type === "stdout") {
					child.stdout.emit("data", bufferLike(event.data));
					setTimeout(poll, 0);
					return;
				}
				if (event.type === "stderr") {
					child.stderr.emit("data", bufferLike(event.data));
					setTimeout(poll, 0);
					return;
				}
				if (event.type === "exit") {
					child.exitCode = event.exitCode;
					child.signalCode = event.signal;
					child.emit("exit", event.exitCode, event.signal);
					child.emit("close", event.exitCode, event.signal);
				}
			};
			queueMicrotask(() => {
				child.emit("spawn");
				poll();
			});
			return child;
		}
		function spawnSync(command, argsOrOptions, maybeOptions) {
			const { args, options } = normalizeArgs(argsOrOptions, maybeOptions);
			try {
				const raw = callSync(
					globalThis._childProcessSpawnSync,
					{
						command: String(command),
						args: args.map(String),
						options: {
							cwd: options.cwd || (globalThis.process && globalThis.process.cwd ? globalThis.process.cwd() : "/"),
							env: options.env,
							input: encodeBytes(options.input),
						},
					},
				);
				const result = typeof raw === "string" ? JSON.parse(raw) : raw;
				const stdout = options.encoding === "utf8" || options.encoding === "utf-8" ? result.stdout : new TextEncoder().encode(result.stdout || "");
				const stderr = options.encoding === "utf8" || options.encoding === "utf-8" ? result.stderr : new TextEncoder().encode(result.stderr || "");
				return {
					pid: -1,
					output: [null, stdout, stderr],
					stdout,
					stderr,
					status: result.code,
					signal: null,
					error: undefined,
				};
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				const stderr = options.encoding === "utf8" || options.encoding === "utf-8" ? message : new TextEncoder().encode(message);
				return {
					pid: -1,
					output: [null, "", stderr],
					stdout: options.encoding === "utf8" || options.encoding === "utf-8" ? "" : new Uint8Array(0),
					stderr,
					status: 1,
					signal: null,
					error,
				};
			}
		}
		module.exports = { spawn, spawnSync, default: { spawn, spawnSync } };
	`,
	"node:child_process": "module.exports = require('child_process');",
	dns: `
		const callAsync = (ref, ...args) => {
			if (typeof ref === "function") return Promise.resolve(ref(...args));
			if (ref && typeof ref.apply === "function") return ref.apply(undefined, args);
			throw new Error("dns bridge is not configured");
		};
		const normalizeLookup = (hostname, options, callback) => {
			let done = callback;
			let normalized = {};
			if (typeof options === "function") {
				done = options;
			} else if (typeof options === "number") {
				normalized.family = options;
			} else if (options && typeof options === "object") {
				normalized = { ...options };
			}
			const family = normalized.family === 4 || normalized.family === 6 ? normalized.family : undefined;
			return {
				callback: done,
				options: {
					hostname: String(hostname),
					family,
					all: normalized.all === true,
				},
			};
		};
		const parseLookupRecords = (resultJson) => {
			let parsed = resultJson;
			if (typeof parsed === "string") parsed = JSON.parse(parsed);
			if (parsed && typeof parsed === "object" && Array.isArray(parsed.records)) parsed = parsed.records;
			else if (parsed && typeof parsed === "object" && typeof parsed.address === "string") parsed = [parsed];
			if (!Array.isArray(parsed)) return [];
			return parsed
				.filter((record) => record && typeof record.address === "string")
				.map((record) => ({ address: record.address, family: record.family === 6 ? 6 : 4 }));
		};
		const lookupRecords = (hostname, options, callback) => {
			const invocation = normalizeLookup(hostname, options, callback);
			return callAsync(globalThis._networkDnsLookupRaw, invocation.options)
				.then(parseLookupRecords)
				.then((records) => {
					if (typeof invocation.callback === "function") {
						if (invocation.options.all) invocation.callback(null, records);
						else {
							const first = records[0] || { address: null, family: invocation.options.family || 0 };
							invocation.callback(null, first.address, first.family);
						}
					}
					return invocation.options.all ? records : records[0] || { address: "", family: invocation.options.family || 0 };
				})
				.catch((error) => {
					if (typeof invocation.callback === "function") {
						invocation.callback(error);
						return undefined;
					}
					throw error;
				});
		};
		const promises = { lookup: (hostname, options) => lookupRecords(hostname, options) };
		function lookup(hostname, options, callback) {
			lookupRecords(hostname, options, callback);
		}
		module.exports = { lookup, promises, default: { lookup, promises } };
	`,
	"dns/promises": "module.exports = require('dns').promises;",
	dgram: `
		const encoder = new TextEncoder();
		const decoder = new TextDecoder();
		const callSync = (ref, ...args) => {
			if (typeof ref === "function") return ref(...args);
			if (ref && typeof ref.applySync === "function") return ref.applySync(undefined, args);
			if (ref && typeof ref.applySyncPromise === "function") return ref.applySyncPromise(undefined, args);
			throw new Error("dgram bridge is not configured");
		};
		const parseResult = (value) => {
			if (typeof value !== "string") return value;
			try { return JSON.parse(value); } catch { return value; }
		};
		const listenersFor = (map, event) => map.get(event) || [];
		const normalizeType = (optionsOrType) => {
			const type = typeof optionsOrType === "string" ? optionsOrType : optionsOrType && optionsOrType.type;
			if (type === "udp6") return "udp6";
			if (type === "udp4" || type === undefined) return "udp4";
			const error = new TypeError("Bad socket type specified. Valid types are: udp4, udp6");
			error.code = "ERR_SOCKET_BAD_TYPE";
			throw error;
		};
		const normalizePort = (port) => {
			const value = Number(port);
			if (!Number.isInteger(value) || value < 0 || value > 65535) {
				const error = new RangeError("Port should be >= 0 and < 65536");
				error.code = "ERR_SOCKET_BAD_PORT";
				throw error;
			}
			return value;
		};
		const normalizeMessage = (value) => {
			if (typeof value === "string") return encoder.encode(value);
			if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
			if (value instanceof ArrayBuffer) return new Uint8Array(value);
			if (Array.isArray(value)) {
				const parts = value.map(normalizeMessage);
				const total = parts.reduce((sum, part) => sum + part.byteLength, 0);
				const output = new Uint8Array(total);
				let offset = 0;
				for (const part of parts) {
					output.set(part, offset);
					offset += part.byteLength;
				}
				return output;
			}
			return encoder.encode(String(value ?? ""));
		};
		const messageBytes = (value) => {
			let bytes;
			if (value && typeof value === "object" && value.__agentOSType === "bytes" && typeof value.base64 === "string") {
				bytes = globalThis.__agentOSEncoding.base64ToBytes(value.base64);
			} else {
				bytes = normalizeMessage(value);
			}
			Object.defineProperty(bytes, "toString", {
				value() { return decoder.decode(bytes); },
				configurable: true,
			});
			return bytes;
		};
		class Socket {
			constructor(optionsOrType, callback) {
				this._type = normalizeType(optionsOrType);
				this._listeners = new Map();
				this._onceListeners = new Map();
				this._closed = false;
				this._bound = false;
				this._polling = false;
				const created = parseResult(callSync(globalThis._dgramSocketCreateRaw, { type: this._type }));
				this._socketId = String(created && created.socketId !== undefined ? created.socketId : created);
				if (typeof callback === "function") this.on("message", callback);
			}
			on(event, listener) {
				const list = listenersFor(this._listeners, event).slice();
				list.push(listener);
				this._listeners.set(event, list);
				return this;
			}
			addListener(event, listener) { return this.on(event, listener); }
			once(event, listener) {
				const list = listenersFor(this._onceListeners, event).slice();
				list.push(listener);
				this._onceListeners.set(event, list);
				return this;
			}
			off(event, listener) { return this.removeListener(event, listener); }
			removeListener(event, listener) {
				this._listeners.set(event, listenersFor(this._listeners, event).filter((entry) => entry !== listener));
				this._onceListeners.set(event, listenersFor(this._onceListeners, event).filter((entry) => entry !== listener));
				return this;
			}
			_emit(event, ...args) {
				for (const listener of listenersFor(this._listeners, event).slice()) listener(...args);
				const once = listenersFor(this._onceListeners, event).slice();
				this._onceListeners.delete(event);
				for (const listener of once) listener(...args);
				return once.length > 0 || listenersFor(this._listeners, event).length > 0;
			}
			emit(event, ...args) { return this._emit(event, ...args); }
			bind(...args) {
				let port = 0;
				let address = this._type === "udp6" ? "::" : "0.0.0.0";
				let callback;
				if (typeof args[0] === "object" && args[0] !== null) {
					port = normalizePort(args[0].port ?? 0);
					address = String(args[0].address ?? address);
					callback = args[1];
				} else {
					if (typeof args[0] === "function") callback = args[0];
					else {
						port = normalizePort(args[0] ?? 0);
						if (typeof args[1] === "string") address = args[1];
						callback = typeof args[1] === "function" ? args[1] : args[2];
					}
				}
				try {
					parseResult(callSync(globalThis._dgramSocketBindRaw, this._socketId, { port, address }));
					this._bound = true;
					queueMicrotask(() => {
						this._emit("listening");
						if (typeof callback === "function") callback.call(this);
						this._poll();
					});
				} catch (error) {
					queueMicrotask(() => this._emit("error", error));
				}
				return this;
			}
			address() {
				return parseResult(callSync(globalThis._dgramSocketAddressRaw, this._socketId));
			}
			send(message, ...args) {
				let offset = 0;
				let length;
				let port;
				let address;
				let callback;
				if (typeof args[0] === "number" && typeof args[1] === "number" && typeof args[2] === "number") {
					offset = args[0];
					length = args[1];
					port = args[2];
					address = typeof args[3] === "string" ? args[3] : undefined;
					callback = typeof args[3] === "function" ? args[3] : args[4];
				} else {
					port = args[0];
					address = typeof args[1] === "string" ? args[1] : undefined;
					callback = typeof args[1] === "function" ? args[1] : args[2];
				}
				const full = normalizeMessage(message);
				const data = length === undefined ? full : full.subarray(offset, offset + length);
				try {
					const result = parseResult(callSync(globalThis._dgramSocketSendRaw, this._socketId, data, {
						port: normalizePort(port),
						address: address || (this._type === "udp6" ? "::1" : "127.0.0.1"),
					}));
					if (typeof callback === "function") queueMicrotask(() => callback(null, result && typeof result.bytes === "number" ? result.bytes : data.length));
				} catch (error) {
					if (typeof callback === "function") queueMicrotask(() => callback(error));
					else queueMicrotask(() => this._emit("error", error));
				}
			}
			_poll() {
				if (this._closed || !this._bound || this._polling) return;
				this._polling = true;
				try {
					const event = parseResult(callSync(globalThis._dgramSocketRecvRaw, this._socketId, 10));
					if (event && event.type === "message") {
						const message = messageBytes({ __agentOSType: "bytes", base64: String(event.data || "") });
						this._emit("message", message, {
							address: event.remoteAddress,
							port: event.remotePort,
							family: event.remoteFamily || (String(event.remoteAddress).includes(":") ? "IPv6" : "IPv4"),
							size: message.length,
						});
					}
				} catch (error) {
					this._emit("error", error);
				} finally {
					this._polling = false;
				}
				if (!this._closed && this._bound) setTimeout(() => this._poll(), 10);
			}
			close(callback) {
				if (typeof callback === "function") this.once("close", callback);
				if (this._closed) return this;
				this._closed = true;
				callSync(globalThis._dgramSocketCloseRaw, this._socketId);
				queueMicrotask(() => this._emit("close"));
				return this;
			}
			ref() { return this; }
			unref() { return this; }
			setRecvBufferSize(size) { callSync(globalThis._dgramSocketSetBufferSizeRaw, this._socketId, "recv", Number(size)); }
			setSendBufferSize(size) { callSync(globalThis._dgramSocketSetBufferSizeRaw, this._socketId, "send", Number(size)); }
			getRecvBufferSize() { return Number(callSync(globalThis._dgramSocketGetBufferSizeRaw, this._socketId, "recv")); }
			getSendBufferSize() { return Number(callSync(globalThis._dgramSocketGetBufferSizeRaw, this._socketId, "send")); }
		}
		function createSocket(optionsOrType, callback) {
			return new Socket(optionsOrType, callback);
		}
		module.exports = { Socket, createSocket, default: { Socket, createSocket } };
	`,
	"node:dgram": "module.exports = require('dgram');",
	crypto: `
		const callSync = (ref, ...args) => {
			if (typeof ref === "function") return ref(...args);
			if (ref && typeof ref.applySync === "function") return ref.applySync(undefined, args);
			if (ref && typeof ref.applySyncPromise === "function") return ref.applySyncPromise(undefined, args);
			throw new Error("crypto bridge is not configured");
		};
		const encoder = new TextEncoder();
		const decoder = new TextDecoder();
		const toBytes = globalThis.__agentOSEncoding.toBytes;
		const concat = (chunks) => {
			const total = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
			const out = new Uint8Array(total);
			let offset = 0;
			for (const chunk of chunks) {
				out.set(chunk, offset);
				offset += chunk.byteLength;
			}
			return out;
		};
		const toHex = (bytes) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
		const SUPPORTED_CIPHERS = ["aes-128-cbc", "aes-128-ctr", "aes-128-gcm", "aes-192-cbc", "aes-192-ctr", "aes-192-gcm", "aes-256-cbc", "aes-256-ctr", "aes-256-gcm", "aes128", "aes192", "aes256"];
		const SUPPORTED_CURVES = ["prime256v1", "secp256k1", "secp384r1", "secp521r1"];
		const toBase64 = globalThis.__agentOSEncoding.bytesToBase64;
		const encodeOutput = (bytes, encoding) => {
			if (!encoding) return makeBuffer(bytes);
			if (encoding === "hex") return toHex(bytes);
			if (encoding === "base64") return toBase64(bytes);
			if (encoding === "utf8" || encoding === "utf-8") return decoder.decode(bytes);
			throw new Error("Unsupported crypto output encoding: " + encoding);
		};
		const makeBuffer = (bytes) => {
			if (typeof Buffer === "function") return Buffer.from(bytes);
			const out = new Uint8Array(bytes);
			out.toString = (encoding = "utf8") => encodeOutput(out, encoding);
			out.equals = (other) => {
				const rhs = toBytes(other);
				if (rhs.byteLength !== out.byteLength) return false;
				for (let i = 0; i < out.byteLength; i += 1) {
					if (out[i] !== rhs[i]) return false;
				}
				return true;
			};
			return out;
		};
		class Hash {
			constructor(algorithm) {
				this.algorithm = String(algorithm);
				this.chunks = [];
			}
			update(data, inputEncoding) {
				this.chunks.push(toBytes(data, inputEncoding));
				return this;
			}
			digest(encoding) {
				const bytes = callSync(globalThis._cryptoHashDigest, this.algorithm, concat(this.chunks));
				return encodeOutput(bytes, encoding);
			}
		}
		class Hmac {
			constructor(algorithm, key) {
				this.algorithm = String(algorithm);
				this.key = toBytes(key);
				this.chunks = [];
			}
			update(data, inputEncoding) {
				this.chunks.push(toBytes(data, inputEncoding));
				return this;
			}
			digest(encoding) {
				const bytes = callSync(globalThis._cryptoHmacDigest, this.algorithm, this.key, concat(this.chunks));
				return encodeOutput(bytes, encoding);
			}
		}
		const CRYPTO_CONSTANTS = {
			RSA_PKCS1_PADDING: 1,
			RSA_PKCS1_OAEP_PADDING: 4,
		};
		// The browser backend signs/verifies with PKCS#1 v1.5 only. Native
		// (OpenSSL) also supports RSA-PSS; rather than silently downgrade a PSS
		// request to PKCS1 (a divergence producing a different, wrong signature),
		// fail loud so the caller sees an explicit unsupported error.
		const assertSupportedSignatureKey = (key) => {
			if (key && typeof key === "object" && !ArrayBuffer.isView(key)) {
				const requestsPss =
					(key.padding !== undefined &&
						key.padding !== CRYPTO_CONSTANTS.RSA_PKCS1_PADDING) ||
					key.saltLength !== undefined;
				if (requestsPss) {
					const error = new Error(
						"ERR_UNSUPPORTED_BROWSER_CRYPTO: RSA-PSS / non-PKCS1 signature padding is not supported on the browser backend",
					);
					error.code = "ERR_UNSUPPORTED_BROWSER_CRYPTO";
					throw error;
				}
			}
		};
		const normalizeKeyInput = (key) => {
			if (typeof key === "string") return key;
			if (key && typeof key === "object" && typeof key.export === "function") return key.export({ format: "pem" });
			if (key && typeof key === "object" && typeof key.key === "string") return key.key;
			if (key && typeof key === "object" && key.key && typeof key.key.export === "function") return key.key.export({ format: "pem" });
			throw new Error("Browser node:crypto RSA key must be a PEM string");
		};
		const normalizeAsymmetricOptions = (keyOrOptions) => {
			if (typeof keyOrOptions === "string") return { key: keyOrOptions };
			if (keyOrOptions && typeof keyOrOptions === "object" && typeof keyOrOptions.export === "function") return { key: keyOrOptions };
			if (keyOrOptions && typeof keyOrOptions === "object") return keyOrOptions;
			throw new Error("Browser node:crypto RSA key must be a PEM string");
		};
		class KeyObject {
			constructor(type, key) {
				this.type = type;
				if (type === "secret") {
					this.symmetricKeySize = toBytes(key).byteLength;
					this.key = new Uint8Array(toBytes(key));
				} else if (key && typeof key === "object" && key.asymmetricKeyType === "x25519") {
					this.asymmetricKeyType = "x25519";
					this.key = new Uint8Array(toBytes(key.key));
					this.publicKey = key.publicKey ? new Uint8Array(toBytes(key.publicKey)) : undefined;
				} else {
					this.asymmetricKeyType = "rsa";
					this.key = normalizeKeyInput(key);
				}
			}
			export(options = {}) {
				if (this.type === "secret") {
					return makeBuffer(this.key);
				}
				if (this.asymmetricKeyType === "x25519") {
					throw new Error("Browser node:crypto X25519 KeyObject export is not implemented yet");
				}
				if (!options || options.format == null || options.format === "pem") return this.key;
				throw new Error("Browser node:crypto KeyObject only supports PEM export");
			}
		}
		class Sign {
			constructor(algorithm) {
				this.algorithm = String(algorithm);
				this.chunks = [];
			}
			update(data, inputEncoding) {
				this.chunks.push(toBytes(data, inputEncoding));
				return this;
			}
			write(data, inputEncoding) {
				this.update(data, inputEncoding);
				return true;
			}
			end(data, inputEncoding) {
				if (data !== undefined) this.update(data, inputEncoding);
				return this;
			}
			sign(key, outputEncoding) {
				assertSupportedSignatureKey(key);
				const bytes = callSync(globalThis._cryptoSign, this.algorithm, concat(this.chunks), normalizeKeyInput(key));
				return encodeOutput(bytes, outputEncoding);
			}
		}
		class Verify extends Sign {
			verify(key, signature, signatureEncoding) {
				assertSupportedSignatureKey(key);
				return Boolean(callSync(
					globalThis._cryptoVerify,
					this.algorithm,
					concat(this.chunks),
					normalizeKeyInput(key),
					toBytes(signature, signatureEncoding),
				));
			}
		}
		function createPrivateKey(key) {
			return new KeyObject("private", key);
		}
		function createPublicKey(key) {
			return new KeyObject("public", key);
		}
		function createSecretKey(key) {
			return new KeyObject("secret", toBytes(key));
		}
		function signOneShot(algorithm, data, key) {
			const signer = new Sign(algorithm);
			signer.update(data);
			return signer.sign(key);
		}
		function verifyOneShot(algorithm, data, key, signature) {
			const verifier = new Verify(algorithm);
			verifier.update(data);
			return verifier.verify(key, signature);
		}
		function modInverse(value, modulus) {
			let t = 0n;
			let newT = 1n;
			let r = modulus;
			let newR = mod(value, modulus);
			while (newR !== 0n) {
				const quotient = r / newR;
				const nextT = t - quotient * newT;
				t = newT;
				newT = nextT;
				const nextR = r - quotient * newR;
				r = newR;
				newR = nextR;
			}
			if (r !== 1n) throw new Error("Browser node:crypto RSA values are not invertible");
			return t < 0n ? t + modulus : t;
		}
		function gcd(left, right) {
			let a = left < 0n ? -left : left;
			let b = right < 0n ? -right : right;
			while (b !== 0n) {
				const next = a % b;
				a = b;
				b = next;
			}
			return a;
		}
		function derLength(length) {
			if (length < 0x80) return new Uint8Array([length]);
			const bytes = [];
			let remaining = length;
			while (remaining > 0) {
				bytes.unshift(remaining & 0xff);
				remaining >>= 8;
			}
			return new Uint8Array([0x80 | bytes.length, ...bytes]);
		}
		function der(tag, content) {
			return concat([new Uint8Array([tag]), derLength(content.byteLength), content]);
		}
		function derInteger(value) {
			let bytes = bigIntToMinimalBytes(value);
			if ((bytes[0] & 0x80) !== 0) bytes = concat([new Uint8Array([0]), bytes]);
			return der(0x02, bytes);
		}
		function derSequence(items) {
			return der(0x30, concat(items));
		}
		function derOctetString(bytes) {
			return der(0x04, bytes);
		}
		function derBitString(bytes) {
			return der(0x03, concat([new Uint8Array([0]), bytes]));
		}
		function derNull() {
			return new Uint8Array([0x05, 0x00]);
		}
		function derObjectIdentifier(parts) {
			const out = [parts[0] * 40 + parts[1]];
			for (const part of parts.slice(2)) {
				const stack = [part & 0x7f];
				let remaining = part >> 7;
				while (remaining > 0) {
					stack.unshift(0x80 | (remaining & 0x7f));
					remaining >>= 7;
				}
				out.push(...stack);
			}
			return der(0x06, new Uint8Array(out));
		}
		const RSA_ENCRYPTION_ALGORITHM = derSequence([
			derObjectIdentifier([1, 2, 840, 113549, 1, 1, 1]),
			derNull(),
		]);
		function pem(label, derBytes) {
			const body = toBase64(derBytes).replace(/.{1,64}/g, "$&\\n").trimEnd();
			return "-----BEGIN " + label + "-----\\n" + body + "\\n-----END " + label + "-----";
		}
		function normalizePublicExponent(value) {
			if (value === undefined) return 65537n;
			if (typeof value === "number") return BigInt(value);
			if (typeof value === "bigint") return value;
			return bytesToBigInt(toBytes(value));
		}
		function encodeRsaPublicKeyDer(key) {
			return derSequence([derInteger(key.n), derInteger(key.e)]);
		}
		function encodeRsaPrivateKeyDer(key) {
			return derSequence([
				derInteger(0n),
				derInteger(key.n),
				derInteger(key.e),
				derInteger(key.d),
				derInteger(key.p),
				derInteger(key.q),
				derInteger(key.d % (key.p - 1n)),
				derInteger(key.d % (key.q - 1n)),
				derInteger(modInverse(key.q, key.p)),
			]);
		}
		function encodeRsaSpkiDer(key) {
			return derSequence([RSA_ENCRYPTION_ALGORITHM, derBitString(encodeRsaPublicKeyDer(key))]);
		}
		function encodeRsaPkcs8Der(key) {
			return derSequence([
				derInteger(0n),
				RSA_ENCRYPTION_ALGORITHM,
				derOctetString(encodeRsaPrivateKeyDer(key)),
			]);
		}
		function encodeGeneratedRsaKey(key, encoding, defaultType) {
			if (!encoding) {
				return defaultType === "public"
					? new KeyObject("public", pem("PUBLIC KEY", encodeRsaSpkiDer(key)))
					: new KeyObject("private", pem("PRIVATE KEY", encodeRsaPkcs8Der(key)));
			}
			const format = encoding.format || "pem";
			const type = encoding.type || (defaultType === "public" ? "spki" : "pkcs8");
			let derBytes;
			let label;
			if (defaultType === "public" && type === "spki") {
				derBytes = encodeRsaSpkiDer(key);
				label = "PUBLIC KEY";
			} else if (defaultType === "public" && (type === "pkcs1" || type === "rsa")) {
				derBytes = encodeRsaPublicKeyDer(key);
				label = "RSA PUBLIC KEY";
			} else if (defaultType === "private" && type === "pkcs8") {
				derBytes = encodeRsaPkcs8Der(key);
				label = "PRIVATE KEY";
			} else if (defaultType === "private" && (type === "pkcs1" || type === "rsa")) {
				derBytes = encodeRsaPrivateKeyDer(key);
				label = "RSA PRIVATE KEY";
			} else {
				throw new Error("Browser node:crypto unsupported RSA key encoding type");
			}
			if (format === "der") return makeBuffer(derBytes);
			if (format === "pem") return pem(label, derBytes);
			throw new Error("Browser node:crypto unsupported RSA key encoding format");
		}
		function generateRsaKeyPair(options = {}) {
			const modulusLength = Number(options.modulusLength || 2048);
			if (!Number.isInteger(modulusLength) || modulusLength < 512) {
				throw new Error("Browser node:crypto RSA modulusLength must be at least 512 bits");
			}
			const e = normalizePublicExponent(options.publicExponent);
			const pBits = Math.floor(modulusLength / 2);
			const qBits = modulusLength - pBits;
			while (true) {
				const p = generatePrimeSync(pBits, { bigint: true });
				const q = generatePrimeSync(qBits, { bigint: true });
				if (p === q) continue;
				const phi = (p - 1n) * (q - 1n);
				if (gcd(e, phi) !== 1n) continue;
				const n = p * q;
				if (n.toString(2).length !== modulusLength) continue;
				const d = modInverse(e, phi);
				const key = { n, e, d, p, q };
				return {
					publicKey: encodeGeneratedRsaKey(key, options.publicKeyEncoding, "public"),
					privateKey: encodeGeneratedRsaKey(key, options.privateKeyEncoding, "private"),
				};
			}
		}
		const X25519_PRIME = (1n << 255n) - 19n;
		const X25519_A24 = 121665n;
		const X25519_BASE_POINT = new Uint8Array([9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
		function mod(value, modulus) {
			const result = value % modulus;
			return result < 0n ? result + modulus : result;
		}
		function bytesToLittleEndianBigInt(bytes) {
			let value = 0n;
			for (let i = bytes.byteLength - 1; i >= 0; i -= 1) {
				value = (value << 8n) | BigInt(bytes[i]);
			}
			return value;
		}
		function littleEndianBigIntToBytes(value, byteLength) {
			const out = new Uint8Array(byteLength);
			let cursor = BigInt(value);
			for (let i = 0; i < byteLength; i += 1) {
				out[i] = Number(cursor & 0xffn);
				cursor >>= 8n;
			}
			return out;
		}
		function normalizeX25519PrivateKey(key) {
			if (!key || key.type !== "private" || key.asymmetricKeyType !== "x25519" || key.key.byteLength !== 32) {
				throw new Error("Browser node:crypto diffieHellman requires an X25519 private KeyObject");
			}
			return key.key;
		}
		function normalizeX25519PublicKey(key) {
			if (!key || key.type !== "public" || key.asymmetricKeyType !== "x25519" || key.key.byteLength !== 32) {
				throw new Error("Browser node:crypto diffieHellman requires an X25519 public KeyObject");
			}
			return key.key;
		}
		function x25519(privateKey, publicKey) {
			const scalarBytes = new Uint8Array(privateKey);
			scalarBytes[0] &= 248;
			scalarBytes[31] &= 127;
			scalarBytes[31] |= 64;
			const uBytes = new Uint8Array(publicKey);
			uBytes[31] &= 127;
			const scalar = bytesToLittleEndianBigInt(scalarBytes);
			const x1 = bytesToLittleEndianBigInt(uBytes);
			let x2 = 1n;
			let z2 = 0n;
			let x3 = x1;
			let z3 = 1n;
			let swap = 0n;
			const cswap = (bit) => {
				if (bit === 0n) return;
				let tmp = x2;
				x2 = x3;
				x3 = tmp;
				tmp = z2;
				z2 = z3;
				z3 = tmp;
			};
			for (let t = 254; t >= 0; t -= 1) {
				const bit = (scalar >> BigInt(t)) & 1n;
				swap ^= bit;
				cswap(swap);
				swap = bit;
				const a = mod(x2 + z2, X25519_PRIME);
				const aa = mod(a * a, X25519_PRIME);
				const b = mod(x2 - z2, X25519_PRIME);
				const bb = mod(b * b, X25519_PRIME);
				const e = mod(aa - bb, X25519_PRIME);
				const c = mod(x3 + z3, X25519_PRIME);
				const d = mod(x3 - z3, X25519_PRIME);
				const da = mod(d * a, X25519_PRIME);
				const cb = mod(c * b, X25519_PRIME);
				x3 = mod((da + cb) * (da + cb), X25519_PRIME);
				z3 = mod(x1 * mod((da - cb) * (da - cb), X25519_PRIME), X25519_PRIME);
				x2 = mod(aa * bb, X25519_PRIME);
				z2 = mod(e * mod(aa + X25519_A24 * e, X25519_PRIME), X25519_PRIME);
			}
			cswap(swap);
			const result = mod(x2 * modPow(z2, X25519_PRIME - 2n, X25519_PRIME), X25519_PRIME);
			return littleEndianBigIntToBytes(result, 32);
		}
		function generateKeyPairSync(type, options = {}) {
			const keyType = String(type).toLowerCase();
			if (keyType === "rsa") {
				return generateRsaKeyPair(options || {});
			}
			if (keyType !== "x25519") {
				return unsupportedBrowserCrypto("generateKeyPairSync");
			}
			const privateBytes = new Uint8Array(callSync(globalThis._cryptoRandomFill, 32));
			const publicBytes = x25519(privateBytes, X25519_BASE_POINT);
			return {
				publicKey: new KeyObject("public", { asymmetricKeyType: "x25519", key: publicBytes }),
				privateKey: new KeyObject("private", { asymmetricKeyType: "x25519", key: privateBytes, publicKey: publicBytes }),
			};
		}
		function generateKeyPair(type, options, callback) {
			if (typeof options === "function") {
				callback = options;
				options = {};
			}
			if (typeof callback !== "function") {
				throw new TypeError('The "callback" argument must be of type function');
			}
			queueMicrotask(() => {
				try {
					const pair = generateKeyPairSync(type, options || {});
					callback(null, pair.publicKey, pair.privateKey);
				} catch (error) {
					callback(error);
				}
			});
		}
		function diffieHellman(options) {
			if (!options || typeof options !== "object") {
				throw new TypeError("Browser node:crypto diffieHellman options must be an object");
			}
			const privateKey = normalizeX25519PrivateKey(options.privateKey);
			const publicKey = normalizeX25519PublicKey(options.publicKey);
			return makeBuffer(x25519(privateKey, publicKey));
		}
		const P256_P = BigInt("0xffffffff00000001000000000000000000000000ffffffffffffffffffffffff");
		const P256_A = P256_P - 3n;
		const P256_B = BigInt("0x5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b");
		const P256_N = BigInt("0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551");
		const P256_G = {
			x: BigInt("0x6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296"),
			y: BigInt("0x4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5"),
		};
		function p256Inverse(value) {
			return modPow(mod(value, P256_P), P256_P - 2n, P256_P);
		}
		function p256PointAdd(left, right) {
			if (!left) return right;
			if (!right) return left;
			if (left.x === right.x) {
				if (mod(left.y + right.y, P256_P) === 0n) return null;
				const slope = mod((3n * left.x * left.x + P256_A) * p256Inverse(2n * left.y), P256_P);
				const x = mod(slope * slope - 2n * left.x, P256_P);
				const y = mod(slope * (left.x - x) - left.y, P256_P);
				return { x, y };
			}
			const slope = mod((right.y - left.y) * p256Inverse(right.x - left.x), P256_P);
			const x = mod(slope * slope - left.x - right.x, P256_P);
			const y = mod(slope * (left.x - x) - left.y, P256_P);
			return { x, y };
		}
		function p256ScalarMult(scalar, point) {
			let result = null;
			let addend = point;
			let remaining = scalar;
			while (remaining > 0n) {
				if ((remaining & 1n) === 1n) result = p256PointAdd(result, addend);
				addend = p256PointAdd(addend, addend);
				remaining >>= 1n;
			}
			return result;
		}
		function p256RandomScalar() {
			while (true) {
				const scalar = bytesToBigInt(callSync(globalThis._cryptoRandomFill, 32)) % P256_N;
				if (scalar > 0n) return scalar;
			}
		}
		function p256EncodePoint(point, format = "uncompressed") {
			if (!point) throw new Error("Browser node:crypto ECDH point is not available");
			if (format === "compressed") {
				const out = new Uint8Array(33);
				out[0] = point.y & 1n ? 0x03 : 0x02;
				out.set(bigIntToBytes(point.x, 32), 1);
				return out;
			}
			if (format !== "uncompressed" && format !== "hybrid") {
				throw new Error("Browser node:crypto ECDH only supports uncompressed, compressed, and hybrid public keys");
			}
			const out = new Uint8Array(65);
			out[0] = format === "hybrid" ? (point.y & 1n ? 0x07 : 0x06) : 0x04;
			out.set(bigIntToBytes(point.x, 32), 1);
			out.set(bigIntToBytes(point.y, 32), 33);
			return out;
		}
		function p256DecodePoint(value, encoding) {
			const bytes = toBytes(value, encoding);
			if (bytes.byteLength !== 65 || (bytes[0] !== 0x04 && bytes[0] !== 0x06 && bytes[0] !== 0x07)) {
				throw new Error("Browser node:crypto ECDH peer public key must be an uncompressed P-256 point");
			}
			const x = bytesToBigInt(bytes.subarray(1, 33));
			const y = bytesToBigInt(bytes.subarray(33, 65));
			if (mod(y * y - (x * x * x + P256_A * x + P256_B), P256_P) !== 0n) {
				throw new Error("Browser node:crypto ECDH peer public key is not on P-256");
			}
			return { x, y };
		}
		class ECDH {
			constructor(name) {
				const curve = String(name);
				if (curve !== "prime256v1" && curve !== "P-256") {
					const error = new Error("Invalid EC curve name");
					error.code = "ERR_CRYPTO_INVALID_CURVE";
					throw error;
				}
				this.privateKey = null;
				this.publicPoint = null;
			}
			generateKeys(encoding, format = "uncompressed") {
				this.privateKey = p256RandomScalar();
				this.publicPoint = p256ScalarMult(this.privateKey, P256_G);
				return encodeOutput(p256EncodePoint(this.publicPoint, format), encoding);
			}
			computeSecret(otherPublicKey, inputEncoding, outputEncoding) {
				if (this.privateKey === null) this.generateKeys();
				const shared = p256ScalarMult(this.privateKey, p256DecodePoint(otherPublicKey, inputEncoding));
				if (!shared) throw new Error("Browser node:crypto ECDH failed to compute shared secret");
				return encodeOutput(bigIntToBytes(shared.x, 32), outputEncoding);
			}
			getPublicKey(encoding, format = "uncompressed") {
				if (!this.publicPoint) throw new Error("Failed to get ECDH public key");
				return encodeOutput(p256EncodePoint(this.publicPoint, format), encoding);
			}
			getPrivateKey(encoding) {
				if (this.privateKey === null) throw new Error("Failed to get ECDH private key");
				return encodeOutput(bigIntToBytes(this.privateKey, 32), encoding);
			}
			setPrivateKey(privateKey, encoding) {
				const scalar = bytesToBigInt(toBytes(privateKey, encoding));
				if (scalar <= 0n || scalar >= P256_N) throw new Error("Invalid ECDH private key");
				this.privateKey = scalar;
				this.publicPoint = p256ScalarMult(this.privateKey, P256_G);
			}
			setPublicKey(publicKey, encoding) {
				this.publicPoint = p256DecodePoint(publicKey, encoding);
			}
		}
		function createECDH(name) {
			return new ECDH(name);
		}
		function generateKeySync(type, options = {}) {
			const keyType = String(type).toLowerCase();
			const length = Number(options && options.length);
			if (!Number.isInteger(length) || length <= 0) {
				throw new Error("Browser node:crypto generateKeySync length must be a positive integer");
			}
			if (keyType === "aes" && ![128, 192, 256].includes(length)) {
				const error = new Error("The property 'options.length' must be one of: 128, 192, 256.");
				error.code = "ERR_INVALID_ARG_VALUE";
				throw error;
			}
			if (keyType !== "hmac" && keyType !== "aes") {
				return unsupportedBrowserCrypto("generateKeySync");
			}
			return createSecretKey(callSync(globalThis._cryptoRandomFill, Math.ceil(length / 8)));
		}
		function bytesToBigInt(bytes) {
			let value = 0n;
			for (const byte of bytes) value = (value << 8n) | BigInt(byte);
			return value;
		}
		function bigIntToBytes(value, byteLength) {
			const out = new Uint8Array(byteLength);
			let cursor = BigInt(value);
			for (let i = byteLength - 1; i >= 0; i -= 1) {
				out[i] = Number(cursor & 0xffn);
				cursor >>= 8n;
			}
			return out;
		}
		function normalizePrimeOption(name, value) {
			if (value === undefined) return undefined;
			if (typeof value === "bigint") return value;
			if (ArrayBuffer.isView(value) || value instanceof ArrayBuffer || Array.isArray(value) || (value && value.type === "Buffer" && Array.isArray(value.data))) {
				return bytesToBigInt(toBytes(value));
			}
			const error = new TypeError('The "options.' + name + '" property must be of type bigint or an instance of ArrayBuffer, TypedArray, Buffer, or DataView.');
			error.code = "ERR_INVALID_ARG_TYPE";
			throw error;
		}
		function modPow(base, exponent, modulus) {
			let result = 1n;
			let cursor = base % modulus;
			let remaining = exponent;
			while (remaining > 0n) {
				if ((remaining & 1n) === 1n) result = (result * cursor) % modulus;
				cursor = (cursor * cursor) % modulus;
				remaining >>= 1n;
			}
			return result;
		}
		const SMALL_PRIMES = [2n, 3n, 5n, 7n, 11n, 13n, 17n, 19n, 23n, 29n, 31n, 37n, 41n, 43n, 47n, 53n, 59n, 61n, 67n, 71n, 73n, 79n, 83n, 89n, 97n];
		const MILLER_RABIN_BASES = [2n, 3n, 5n, 7n, 11n, 13n, 17n, 19n, 23n, 29n, 31n, 37n];
		function isProbablePrime(value) {
			if (value < 2n) return false;
			for (const prime of SMALL_PRIMES) {
				if (value === prime) return true;
				if (value % prime === 0n) return false;
			}
			let d = value - 1n;
			let s = 0;
			while ((d & 1n) === 0n) {
				d >>= 1n;
				s += 1;
			}
			for (const base of MILLER_RABIN_BASES) {
				if (base >= value - 2n) continue;
				let x = modPow(base, d, value);
				if (x === 1n || x === value - 1n) continue;
				let witness = false;
				for (let r = 1; r < s; r += 1) {
					x = (x * x) % value;
					if (x === value - 1n) {
						witness = true;
						break;
					}
				}
				if (!witness) return false;
			}
			return true;
		}
		function randomPrimeCandidate(size, add, rem) {
			const byteLength = Math.ceil(size / 8);
			const mask = (1n << BigInt(size)) - 1n;
			const highBit = 1n << BigInt(size - 1);
			let candidate = (bytesToBigInt(callSync(globalThis._cryptoRandomFill, byteLength)) & mask) | highBit;
			if (add !== undefined) {
				const desired = rem === undefined ? 1n : rem;
				const delta = (desired - (candidate % add) + add) % add;
				candidate += delta;
				if (candidate > mask) candidate -= add;
			} else {
				candidate |= 1n;
			}
			return candidate;
		}
		function generatePrimeSync(size, options = {}) {
			const bitLength = Number(size);
			if (!Number.isInteger(bitLength) || bitLength < 2) {
				throw new RangeError("Browser node:crypto generatePrimeSync size must be an integer greater than 1");
			}
			if (bitLength > 4096) {
				throw new RangeError("Browser node:crypto generatePrimeSync supports primes up to 4096 bits");
			}
			const primeOptions = options || {};
			const add = normalizePrimeOption("add", primeOptions.add);
			const rem = normalizePrimeOption("rem", primeOptions.rem);
			if (add !== undefined && add <= 0n) {
				throw new RangeError("Browser node:crypto generatePrimeSync options.add must be greater than zero");
			}
			if (rem !== undefined && add === undefined) {
				throw new RangeError("Browser node:crypto generatePrimeSync options.rem requires options.add");
			}
			const safe = primeOptions.safe === true;
			while (true) {
				const candidate = randomPrimeCandidate(bitLength, add, rem);
				if (candidate < 2n || candidate.toString(2).length !== bitLength) continue;
				if (!isProbablePrime(candidate)) continue;
				if (safe && !isProbablePrime((candidate - 1n) / 2n)) continue;
				if (primeOptions.bigint === true) return candidate;
				const bytes = bigIntToBytes(candidate, Math.ceil(bitLength / 8));
				return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
			}
		}
		const DIFFIE_HELLMAN_GROUPS = {
			modp14: {
				prime: "ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca18217c32905e462e36ce3be39e772c180e86039b2783a2ec07a28fb5c55df06f4c52c9de2bcbf6955817183995497cea956ae515d2261898fa051015728e5a8aacaa68ffffffffffffffff",
				generator: 2n,
			},
		};
		function bigIntToMinimalBytes(value) {
			if (value === 0n) return new Uint8Array([0]);
			return bigIntToBytes(value, Math.ceil(value.toString(16).length / 2));
		}
		function normalizeDhNumber(value, encoding) {
			if (typeof value === "bigint") return value;
			if (typeof value === "number") return BigInt(value);
			return bytesToBigInt(toBytes(value, encoding));
		}
		class DiffieHellman {
			constructor(prime, generator = 2n) {
				this.prime = BigInt(prime);
				this.generator = BigInt(generator);
				this.primeLength = Math.ceil(this.prime.toString(2).length / 8);
				this.privateKey = null;
				this.publicKey = null;
				this.verifyError = 0;
			}
			_generatePrivateKey() {
				const randomLength = Math.min(this.primeLength, 32);
				const random = bytesToBigInt(callSync(globalThis._cryptoRandomFill, randomLength));
				return 2n + (random % (this.prime - 3n));
			}
			generateKeys(encoding) {
				this.privateKey = this._generatePrivateKey();
				this.publicKey = modPow(this.generator, this.privateKey, this.prime);
				return encodeOutput(bigIntToBytes(this.publicKey, this.primeLength), encoding);
			}
			computeSecret(otherPublicKey, inputEncoding, outputEncoding) {
				if (this.privateKey === null) this.generateKeys();
				const peer = normalizeDhNumber(otherPublicKey, inputEncoding);
				const secret = modPow(peer, this.privateKey, this.prime);
				return encodeOutput(bigIntToBytes(secret, this.primeLength), outputEncoding);
			}
			getPrime(encoding) {
				return encodeOutput(bigIntToBytes(this.prime, this.primeLength), encoding);
			}
			getGenerator(encoding) {
				return encodeOutput(bigIntToMinimalBytes(this.generator), encoding);
			}
			getPublicKey(encoding) {
				if (this.publicKey === null) this.generateKeys();
				return encodeOutput(bigIntToBytes(this.publicKey, this.primeLength), encoding);
			}
			getPrivateKey(encoding) {
				if (this.privateKey === null) this.generateKeys();
				return encodeOutput(bigIntToMinimalBytes(this.privateKey), encoding);
			}
			setPublicKey(key, encoding) {
				this.publicKey = normalizeDhNumber(key, encoding);
			}
			setPrivateKey(key, encoding) {
				this.privateKey = normalizeDhNumber(key, encoding);
				this.publicKey = modPow(this.generator, this.privateKey, this.prime);
			}
		}
		function createDiffieHellman(prime, primeEncoding, generator, generatorEncoding) {
			let normalizedGenerator = generator;
			let normalizedGeneratorEncoding = generatorEncoding;
			if (typeof primeEncoding !== "string") {
				normalizedGenerator = primeEncoding === undefined ? generator : primeEncoding;
				normalizedGeneratorEncoding = typeof generator === "string" ? generator : undefined;
				primeEncoding = undefined;
			}
			const primeValue = normalizeDhNumber(prime, primeEncoding);
			const generatorValue = normalizedGenerator === undefined
				? 2n
				: normalizeDhNumber(normalizedGenerator, normalizedGeneratorEncoding);
			return new DiffieHellman(primeValue, generatorValue);
		}
		function getDiffieHellman(name) {
			const group = DIFFIE_HELLMAN_GROUPS[String(name).toLowerCase()];
			if (!group) {
				const error = new Error("Unknown DH group");
				error.code = "ERR_CRYPTO_UNKNOWN_DH_GROUP";
				throw error;
			}
			return new DiffieHellman(bytesToBigInt(toBytes(group.prime, "hex")), group.generator);
		}
		function publicEncrypt(keyOrOptions, buffer) {
			const options = normalizeAsymmetricOptions(keyOrOptions);
			const bytes = callSync(
				globalThis._cryptoAsymmetricOp,
				"publicEncrypt",
				normalizeKeyInput(options.key),
				toBytes(buffer),
				JSON.stringify({
					padding: options.padding,
					oaepHash: options.oaepHash,
					oaepLabel: options.oaepLabel ? Array.from(toBytes(options.oaepLabel)) : undefined,
				}),
			);
			return makeBuffer(bytes);
		}
		function privateDecrypt(keyOrOptions, buffer) {
			const options = normalizeAsymmetricOptions(keyOrOptions);
			const bytes = callSync(
				globalThis._cryptoAsymmetricOp,
				"privateDecrypt",
				normalizeKeyInput(options.key),
				toBytes(buffer),
				JSON.stringify({
					padding: options.padding,
					oaepHash: options.oaepHash,
					oaepLabel: options.oaepLabel ? Array.from(toBytes(options.oaepLabel)) : undefined,
				}),
			);
			return makeBuffer(bytes);
		}
		function randomBytes(size, callback) {
			const bytes = makeBuffer(callSync(globalThis._cryptoRandomFill, Number(size)));
			if (typeof callback === "function") queueMicrotask(() => callback(null, bytes));
			return bytes;
		}
		function randomFillSync(buffer, offset = 0, size) {
			const view = toBytes(buffer);
			const start = Number(offset) || 0;
			const length = size == null ? view.byteLength - start : Number(size);
			view.set(callSync(globalThis._cryptoRandomFill, length), start);
			return buffer;
		}
		function pbkdf2Sync(password, salt, iterations, keyLength, digest = "sha1") {
			return makeBuffer(callSync(
				globalThis._cryptoPbkdf2,
				toBytes(password),
				toBytes(salt),
				Number(iterations),
				Number(keyLength),
				String(digest),
			));
		}
		function pbkdf2(password, salt, iterations, keyLength, digest, callback) {
			if (typeof digest === "function") {
				callback = digest;
				digest = "sha1";
			}
			queueMicrotask(() => {
				try {
					callback(null, pbkdf2Sync(password, salt, iterations, keyLength, digest || "sha1"));
				} catch (error) {
					callback(error);
				}
			});
		}
		function scryptSync(password, salt, keyLength, options = undefined) {
			return makeBuffer(callSync(
				globalThis._cryptoScrypt,
				toBytes(password),
				toBytes(salt),
				Number(keyLength),
				options || {},
			));
		}
		function scrypt(password, salt, keyLength, options, callback) {
			if (typeof options === "function") {
				callback = options;
				options = undefined;
			}
			if (typeof callback !== "function") {
				throw new TypeError('The "callback" argument must be of type function');
			}
			queueMicrotask(() => {
				try {
					callback(null, scryptSync(password, salt, keyLength, options));
				} catch (error) {
					callback(error);
				}
			});
		}
		class Cipheriv {
			constructor(mode, algorithm, key, iv, options = {}) {
				this.mode = mode;
				this.algorithm = String(algorithm);
				this.key = toBytes(key);
				this.iv = toBytes(iv);
				this.options = { ...(options || {}) };
				this.chunks = [];
				this.finished = false;
				this.authTag = null;
			}
			update(data, inputEncoding, outputEncoding) {
				if (this.finished) throw new Error("Cipheriv final already called");
				this.chunks.push(toBytes(data, inputEncoding));
				return encodeOutput(new Uint8Array(0), outputEncoding);
			}
			final(outputEncoding) {
				if (this.finished) throw new Error("Cipheriv final already called");
				this.finished = true;
				const input = concat(this.chunks);
				let result;
				if (this.mode === "cipher") {
					result = callSync(globalThis._cryptoCipheriv, this.algorithm, this.key, this.iv, input, this.options);
					if (this.algorithm.toLowerCase().endsWith("-gcm")) {
						this.authTag = result.slice(result.byteLength - 16);
						result = result.slice(0, result.byteLength - 16);
					}
				} else {
					result = callSync(globalThis._cryptoDecipheriv, this.algorithm, this.key, this.iv, input, this.options);
				}
				return encodeOutput(result, outputEncoding);
			}
			setAutoPadding(autoPadding = true) {
				if (this.finished) throw new Error("Cipheriv final already called");
				this.options.autoPadding = autoPadding !== false;
				return this;
			}
			setAAD(aad) {
				if (this.finished) throw new Error("Cipheriv final already called");
				this.options.aad = toBytes(aad);
				return this;
			}
			getAuthTag() {
				if (!this.authTag) throw new Error("Cipheriv auth tag is not available");
				return makeBuffer(this.authTag);
			}
			setAuthTag(tag) {
				if (this.finished) throw new Error("Cipheriv final already called");
				this.options.authTag = toBytes(tag);
				return this;
			}
		}
		function unsupportedBrowserCrypto(operation) {
			const error = new Error("node:crypto " + operation + " is not implemented in the browser runtime yet");
			error.code = "ERR_UNSUPPORTED_BROWSER_CRYPTO";
			throw error;
		}
		module.exports = {
			createCipheriv: (algorithm, key, iv, options) => new Cipheriv("cipher", algorithm, key, iv, options),
			createDecipheriv: (algorithm, key, iv, options) => new Cipheriv("decipher", algorithm, key, iv, options),
			createDiffieHellman,
			createECDH,
			createHash: (algorithm) => new Hash(algorithm),
			createHmac: (algorithm, key) => new Hmac(algorithm, key),
			constants: CRYPTO_CONSTANTS,
			createPrivateKey,
			createPublicKey,
			createSecretKey,
			createSign: (algorithm) => new Sign(algorithm),
			createVerify: (algorithm) => new Verify(algorithm),
			diffieHellman,
			generateKeyPair,
			generateKeyPairSync,
			generateKeySync,
			generatePrimeSync,
			getCiphers: () => [...SUPPORTED_CIPHERS],
			getCurves: () => [...SUPPORTED_CURVES],
			getDiffieHellman,
			getHashes: () => ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"],
			pbkdf2,
			pbkdf2Sync,
			privateDecrypt,
			publicEncrypt,
			randomBytes,
			randomFillSync,
			randomUUID: () => callSync(globalThis._cryptoRandomUUID),
			scrypt,
			scryptSync,
			sign: signOneShot,
			subtle: globalThis.crypto && globalThis.crypto.subtle,
			verify: verifyOneShot,
			webcrypto: globalThis.crypto,
		};
	`,
	"node:crypto": "module.exports = require('crypto');",
	wasi: BROWSER_WASI_POLYFILL_CODE,
	"node:wasi": "module.exports = require('wasi');",
	"secure-exec:wasi-command-host": `
		function defaultDecode(bytes) {
			return new TextDecoder().decode(bytes);
		}
		function decodeNullSeparated(bytes) {
			const out = [];
			let start = 0;
			for (let i = 0; i <= bytes.length; i += 1) {
				if (i === bytes.length || bytes[i] === 0) {
					if (i > start) out.push(defaultDecode(bytes.slice(start, i)));
					start = i + 1;
				}
			}
			return out;
		}
		function parseEnv(bytes) {
			const env = {};
			for (const entry of decodeNullSeparated(bytes)) {
				const eq = entry.indexOf("=");
				if (eq > 0) env[entry.slice(0, eq)] = entry.slice(eq + 1);
			}
			return env;
		}
		async function readCommandBytes(source) {
			if (source instanceof Uint8Array) return source;
			if (source instanceof ArrayBuffer) return new Uint8Array(source);
			if (source instanceof WebAssembly.Module) return source;
			if (typeof source !== "string") throw new Error("command source must be a URL, bytes, or WebAssembly.Module");
			const response = await fetch(source);
			if (!response.ok) throw new Error("failed to fetch command wasm " + source + ": " + response.status);
			let bytes = new Uint8Array(await response.arrayBuffer());
			if (response.headers && response.headers.get("x-body-encoding") === "base64") {
				const encoded = new TextDecoder().decode(bytes);
				bytes = Uint8Array.from(atob(encoded), (char) => char.charCodeAt(0));
			}
			return bytes;
		}
		async function loadCommandModules(commands) {
			const modules = new Map();
			for (const [name, source] of Object.entries(commands || {})) {
				const value = await readCommandBytes(source);
				modules.set(name, value instanceof WebAssembly.Module ? value : new WebAssembly.Module(value));
			}
			return modules;
		}
		async function createWasiCommandHost(options) {
			const WASI = options && options.WASI ? options.WASI : require("node:wasi").WASI;
			const commandModules = await loadCommandModules(options && options.commands);
			let memory = null;
			let nextPid = 100;
			const exitedChildren = new Map();
			const deferredChildren = new Map();
			const waitBuffer = new SharedArrayBuffer(4);
			const wait = new Int32Array(waitBuffer);
			const errnoSuccess = 0;
			const errnoBadf = 8;
			const errnoChild = 10;
			const errnoNosys = 52;
			let nextSyntheticFd = 1000;
			const syntheticFdEntries = new Map();
			let activeFdOverrides = null;
			let activeChildCwd = null;
			let previousLookupFdHandle = null;
			let parentWasi = null;
			const getMemory = () => {
				if (!memory) throw new Error("WASI host command memory is not set");
				return memory;
			};
			const view = () => new DataView(getMemory().buffer);
			const bytes = () => new Uint8Array(getMemory().buffer);
			const writeU32 = (ptr, value) => {
				view().setUint32(ptr >>> 0, value >>> 0, true);
				return errnoSuccess;
			};
			const writeBytes = (ptr, value) => {
				bytes().set(value, ptr >>> 0);
			};
			const readBytes = (ptr, len) => bytes().slice(ptr >>> 0, (ptr >>> 0) + (len >>> 0));
			const readString = (ptr, len) => defaultDecode(readBytes(ptr, len));
			const fs = () => require("node:fs");
			const path = () => require("node:path");
			const userRecord = new TextEncoder().encode(
				(options && options.userRecord) || "agentos:x:1000:1000:Agent OS:/tmp:/bin/sh",
			);
			const modeFromStat = (stat, fallback) => {
				const mode = Number(stat && stat.mode);
				if (Number.isInteger(mode) && mode > 0) return mode >>> 0;
				if (stat && typeof stat.isDirectory === "function" && stat.isDirectory()) return 0o040755;
				if (stat && typeof stat.isSymbolicLink === "function" && stat.isSymbolicLink()) return 0o120777;
				return fallback >>> 0;
			};
			const currentGuestCwd = () => {
				const cwd = typeof activeChildCwd === "string" && activeChildCwd.startsWith("/")
					? activeChildCwd
					: typeof options?.cwd === "string" && options.cwd.startsWith("/")
					? options.cwd
					: "/";
				return path().posix.normalize(cwd);
			};
			const resolveGuestPath = (target) => {
				const value = String(target || ".");
				return value.startsWith("/")
					? path().posix.normalize(value)
					: path().posix.resolve(currentGuestCwd(), value);
			};
			const lookupSyntheticFd = (fd) => {
				const descriptor = fd >>> 0;
				const override = activeFdOverrides && activeFdOverrides.get(descriptor);
				if (override && override.open !== false) return override;
				const handle = syntheticFdEntries.get(descriptor);
				if (handle && handle.open !== false) return handle;
				if (typeof previousLookupFdHandle === "function") return previousLookupFdHandle(descriptor);
				const parentEntry = parentWasi && parentWasi.fdTable && parentWasi.fdTable.get(descriptor);
				if (parentEntry && parentEntry.kind === "file" && typeof parentEntry.realFd === "number") {
					return {
						kind: "guest-file",
						targetFd: parentEntry.realFd,
						position: typeof parentEntry.offset === "number" ? parentEntry.offset : 0,
						readOnly: parentEntry.readOnly === true,
						open: true,
					};
				}
				return null;
			};
			const closeSyntheticHandle = (handle) => {
				if (!handle || handle.open === false) return;
				handle.open = false;
				if (handle.kind === "pipe-read" && handle.pipe) {
					handle.pipe.readHandleCount = Math.max(0, (handle.pipe.readHandleCount || 0) - 1);
				} else if (handle.kind === "pipe-write" && handle.pipe) {
					handle.pipe.writeHandleCount = Math.max(0, (handle.pipe.writeHandleCount || 0) - 1);
				}
				if (typeof handle.onClose === "function") handle.onClose(handle);
			};
			const cloneSyntheticHandle = (handle) => {
				if (!handle || handle.open === false) return null;
				if (handle.kind === "stdio") {
					return { kind: "stdio", targetFd: handle.targetFd, open: true };
				}
				if (handle.kind === "guest-file") {
					return { ...handle, open: true };
				}
				if (!handle.pipe) return null;
				if (handle.kind === "pipe-read") {
					handle.pipe.readHandleCount = (handle.pipe.readHandleCount || 0) + 1;
					return { kind: "pipe-read", pipe: handle.pipe, open: true, onClose: handle.onClose };
				}
				if (handle.kind === "pipe-write") {
					handle.pipe.writeHandleCount = (handle.pipe.writeHandleCount || 0) + 1;
					return { kind: "pipe-write", pipe: handle.pipe, open: true, onClose: handle.onClose };
				}
				return null;
			};
			const handleMatchesStdio = (handle, expectedKind) => {
				if (!handle || handle.open === false) return false;
				if (handle.kind === "stdio") {
					if (expectedKind === "read") return handle.targetFd === 0;
					if (expectedKind === "write") return handle.targetFd === 1 || handle.targetFd === 2;
				}
				if (expectedKind === "read") return handle.kind === "pipe-read" || handle.kind === "guest-file";
				if (expectedKind === "write") return handle.kind === "pipe-write" || handle.kind === "guest-file";
				return handle.kind === expectedKind;
			};
			const allocateSyntheticFd = (handle) => {
				const fd = nextSyntheticFd++;
				syntheticFdEntries.set(fd, handle);
				return fd;
			};
			const replaceSyntheticFd = (fd, handle) => {
				const descriptor = fd >>> 0;
				closeSyntheticHandle(syntheticFdEntries.get(descriptor));
				syntheticFdEntries.set(descriptor, handle);
			};
			const pipeHasOpenWriters = (handle) =>
				handle && handle.kind === "pipe-read" && handle.pipe && (handle.pipe.writeHandleCount || 0) > 0;
			const runChild = (child) => {
				const parentMemory = memory;
				const previousActiveFdOverrides = activeFdOverrides;
				const previousActiveChildCwd = activeChildCwd;
				try {
					const childWasi = new WASI({
						returnOnExit: true,
						args: [child.commandPath, ...child.argv.slice(1)],
						env: child.env,
						preopens: { "/": child.cwd || "/" },
					});
					const childImports = {
						wasi_snapshot_preview1: childWasi.wasiImport,
						...host.imports,
					};
					const childInstance = new WebAssembly.Instance(child.module, childImports);
					memory = childInstance.exports.memory;
					activeFdOverrides = child.overrides;
					activeChildCwd = child.cwd || "/";
					const exitCode = childWasi.start(childInstance);
					exitedChildren.set(child.pid, exitCode << 8);
				} catch {
					exitedChildren.set(child.pid, 127 << 8);
				} finally {
					for (const handle of child.childOverrideHandles) closeSyntheticHandle(handle);
					activeFdOverrides = previousActiveFdOverrides;
					activeChildCwd = previousActiveChildCwd;
					memory = parentMemory;
				}
			};
			const runReadyDeferredChildren = (requestedPid) => {
				let ran = false;
				for (const [pid, child] of Array.from(deferredChildren.entries())) {
					if (requestedPid && pid !== requestedPid) continue;
					const stdinHandle = child.overrides.get(0);
					if (pipeHasOpenWriters(stdinHandle)) continue;
					deferredChildren.delete(pid);
					runChild(child);
					ran = true;
				}
				return ran;
			};
			const onPipeHandleClose = () => {
				while (runReadyDeferredChildren()) {
					// Keep draining children made ready by the previous child exit.
				}
			};
			const host = {
				setMemory(nextMemory) {
					memory = nextMemory;
					return host;
				},
				setParentWasi(wasi) {
					parentWasi = wasi || null;
					return host;
				},
				installBlockingStdin(processLike) {
					const target = processLike || globalThis.process;
					const wasiHost = globalThis.__agentOSWasiHost || (globalThis.__agentOSWasiHost = {});
					wasiHost.readStdin = (maxBytes) => {
						while (true) {
							const value = target && target.stdin && typeof target.stdin.read === "function"
								? target.stdin.read(maxBytes)
								: null;
							const length = typeof value === "string"
								? value.length
								: value instanceof Uint8Array
									? value.byteLength
									: value && typeof value.byteLength === "number"
										? value.byteLength
										: 0;
							if (length > 0) return value;
							Atomics.wait(wait, 0, 0, 10);
						}
					};
					wasiHost.readStdinNonBlocking = (maxBytes) =>
							target && target.stdin && typeof target.stdin.read === "function"
								? target.stdin.read(maxBytes)
								: null;
						wasiHost.stdinReadableBytes = () => 1;
					if (typeof wasiHost.lookupFdHandle === "function" && wasiHost.lookupFdHandle !== lookupSyntheticFd) {
						previousLookupFdHandle = wasiHost.lookupFdHandle;
					}
					wasiHost.lookupFdHandle = lookupSyntheticFd;
					return host;
				},
				imports: {
					host_tty: {
						// crossterm WasiEventSource keystroke source: read(ptr, len, timeout_ms) -> usize.
						// usize::MAX (-1 as i32) means block until input; the brush/reedline read loop
						// polls with None (blocking), so we wait on the kernel PTY stdin and copy bytes
						// into guest memory, returning the count. Short/zero timeouts report "no event"
						// (0); the guest then falls back to its blocking read.
						read(ptr, len, timeoutMs) {
							const cap = len >>> 0;
							if (cap === 0) return 0;
							const wasiHost = globalThis.__agentOSWasiHost;
							if (!wasiHost) return 0;
							const blocking = (timeoutMs >>> 0) === 0xffffffff;
							const budget = blocking ? Infinity : (timeoutMs >>> 0);
							const toBytes = (value) => {
								if (typeof value === "string") return new TextEncoder().encode(value);
								if (value instanceof Uint8Array) return value;
								if (value && typeof value.byteLength === "number")
									return new Uint8Array(value.buffer || value, value.byteOffset || 0, value.byteLength);
								return null;
							};
							let waited = 0;
							for (;;) {
								// Prefer a single non-blocking read so finite timeouts (e.g. crossterm's
								// cursor-position report) can return promptly with whatever is queued.
								const value = typeof wasiHost.readStdinNonBlocking === "function"
									? wasiHost.readStdinNonBlocking(cap)
									: null;
								const bytes = toBytes(value);
								if (bytes && bytes.length > 0) {
									const n = Math.min(bytes.length, cap);
									writeBytes(ptr, bytes.subarray(0, n));
									return n;
								}
								if (!blocking && waited >= budget) return 0;
								const step = blocking ? 10 : Math.max(1, Math.min(10, budget - waited));
								Atomics.wait(wait, 0, 0, step);
								waited += step;
							}
						},
						// Toggle terminal raw mode on the guest's PTY. crossterm calls this instead
						// of tcsetattr; route it to the kernel via process.stdin.setRawMode (which
						// drives __pty_set_raw_mode), so reedline gets raw \r keystrokes and submits
						// commands. Returns errno 0.
						set_raw_mode(_enabled) {
							return 0;
						},
					},
					host_user: {
						getuid(ret) { return writeU32(ret, 1000); },
						getgid(ret) { return writeU32(ret, 1000); },
						geteuid(ret) { return writeU32(ret, 1000); },
						getegid(ret) { return writeU32(ret, 1000); },
						isatty(fd, ret) {
							return writeU32(ret, fd === 0 || fd === 1 || fd === 2 ? 1 : 0);
						},
						getpwuid(_uid, bufPtr, bufLen, retLen) {
							const len = Math.min(userRecord.length, bufLen >>> 0);
							writeBytes(bufPtr, userRecord.subarray(0, len));
							writeU32(retLen, len);
							return errnoSuccess;
						},
					},
					host_fs: {
						fd_mode(fd) {
							const descriptor = fd >>> 0;
							if (descriptor <= 2) return 0o020666;
							const handle = lookupSyntheticFd(descriptor);
							if (handle && (handle.kind === "pipe-read" || handle.kind === "pipe-write")) return 0o010600;
							if (handle && handle.kind === "guest-file" && typeof handle.targetFd === "number") {
								try {
									return modeFromStat(fs().fstatSync(handle.targetFd), 0o100644);
								} catch {
									return 0o100644;
								}
							}
							const parentEntry = parentWasi && parentWasi.fdTable && parentWasi.fdTable.get(descriptor);
							if (parentEntry && (parentEntry.kind === "preopen" || parentEntry.kind === "directory")) return 0o040755;
							if (parentEntry && parentEntry.kind === "file" && typeof parentEntry.realFd === "number") {
								try {
									return modeFromStat(fs().fstatSync(parentEntry.realFd), 0o100644);
								} catch {
									return 0o100644;
								}
							}
							return 0o100644;
						},
						// Signature must match the node runner's host_fs.path_mode
						// (fd, pathPtr, pathLen, followSymlinks). The guest passes the
						// directory fd first (3 = cwd preopen); the path is at args 2/3.
						path_mode(_fd, pathPtr, pathLen, followSymlinks) {
							try {
								const guestPath = resolveGuestPath(readString(pathPtr, pathLen));
								const stat = Number(followSymlinks) === 0
									? fs().lstatSync(guestPath)
									: fs().statSync(guestPath);
								return modeFromStat(stat, 0o100644);
							} catch {
								return 0;
							}
						},
						// Matches node runner host_fs.chmod(fd, pathPtr, pathLen, mode):
						// 0 on success, 1 on failure.
						chmod(_fd, pathPtr, pathLen, mode) {
							try {
								const guestPath = resolveGuestPath(readString(pathPtr, pathLen));
								fs().chmodSync(guestPath, Number(mode) >>> 0);
								return 0;
							} catch {
								return 1;
							}
						},
						// The node runner exports 7 host_fs symbols; mirror the full
						// contract here so any guest module that imports the rest still
						// instantiates in-browser (a missing import is a hard LinkError).
						// Sentinels match the node runner: (1<<64)-1 for size, 1 for
						// mutations.
						fd_size(fd) {
							try {
								const descriptor = fd >>> 0;
								const handle = lookupSyntheticFd(descriptor);
								if (
									handle &&
									handle.kind === "guest-file" &&
									typeof handle.targetFd === "number"
								) {
									return BigInt(fs().fstatSync(handle.targetFd).size ?? -1);
								}
								const parentEntry =
									parentWasi && parentWasi.fdTable && parentWasi.fdTable.get(descriptor);
								if (
									parentEntry &&
									parentEntry.kind === "file" &&
									typeof parentEntry.realFd === "number"
								) {
									return BigInt(fs().fstatSync(parentEntry.realFd).size ?? -1);
								}
								return (1n << 64n) - 1n;
							} catch {
								return (1n << 64n) - 1n;
							}
						},
						path_size(_fd, pathPtr, pathLen, followSymlinks) {
							try {
								const guestPath = resolveGuestPath(readString(pathPtr, pathLen));
								const stat = Number(followSymlinks) === 0
									? fs().lstatSync(guestPath)
									: fs().statSync(guestPath);
								return BigInt(stat.size ?? -1);
							} catch {
								return (1n << 64n) - 1n;
							}
						},
						// Browser fs() has no fd-based fchmod/ftruncate; provide the
						// symbols (best-effort failure) so imports resolve. Rust guest
						// binaries bypass wasi-libc stat/chmod/truncate, so these paths
						// are unreached in practice.
						fchmod(_fd, _mode) {
							return 1;
						},
						ftruncate(_fd, _length) {
							return 1;
						},
					},
					host_process: {
						proc_spawn(argvPtr, argvLen, envpPtr, envpLen, stdinFd, stdoutFd, stderrFd, cwdPtr, cwdLen, retPid) {
							try {
								const argv = decodeNullSeparated(readBytes(argvPtr, argvLen));
								if (argv.length === 0) return errnoNosys;
								const commandPath = argv[0];
								const commandName = commandPath.split("/").filter(Boolean).at(-1) || commandPath;
								const module = commandModules.get(commandName);
								if (!module) return errnoNosys;
								const env = {
									...(options && options.env ? options.env : {}),
									...parseEnv(readBytes(envpPtr, envpLen)),
									PATH: (options && options.path) || "/bin:/usr/bin",
								};
								const cwd = cwdLen ? readString(cwdPtr, cwdLen) : ((options && options.cwd) || "/");
								const childOverrideHandles = [];
								const overrides = new Map();
								for (const [childFd, parentFd, expectedKind] of [
									[0, stdinFd >>> 0, "read"],
									[1, stdoutFd >>> 0, "write"],
									[2, stderrFd >>> 0, "write"],
								]) {
									const parentHandle = lookupSyntheticFd(parentFd);
									if (parentFd <= 2 && !parentHandle) continue;
									if (!handleMatchesStdio(parentHandle, expectedKind)) return errnoBadf;
									const childHandle = cloneSyntheticHandle(parentHandle);
									if (!childHandle) return errnoBadf;
									overrides.set(childFd, childHandle);
									childOverrideHandles.push(childHandle);
								}
								const pid = nextPid++;
								const child = { pid, module, commandPath, argv, env, cwd, overrides, childOverrideHandles };
								if (pipeHasOpenWriters(overrides.get(0))) {
									deferredChildren.set(pid, child);
								} else {
									runChild(child);
								}
								return writeU32(retPid, pid);
							} catch {
								return errnoNosys;
							}
						},
						proc_waitpid(pid, _options, retStatus, retPid) {
							const requested = pid >>> 0;
							runReadyDeferredChildren(requested === 0xffffffff ? undefined : requested);
							const childPid = requested === 0xffffffff
								? exitedChildren.keys().next().value
								: requested;
							if (!childPid || !exitedChildren.has(childPid)) {
								writeU32(retPid, 0);
								return errnoChild;
							}
							writeU32(retStatus, exitedChildren.get(childPid) || 0);
							writeU32(retPid, childPid);
							exitedChildren.delete(childPid);
							return errnoSuccess;
						},
						fd_dup(fd, retNewFd) {
							const descriptor = fd >>> 0;
							const handle = lookupSyntheticFd(descriptor) || (descriptor <= 2
								? { kind: "stdio", targetFd: descriptor, open: true }
								: null);
							if (!handle) return writeU32(retNewFd, fd);
							const cloned = cloneSyntheticHandle(handle);
							if (!cloned) return errnoBadf;
							return writeU32(retNewFd, allocateSyntheticFd(cloned));
						},
						fd_dup2(oldFd, newFd) {
							if (oldFd === newFd) return errnoSuccess;
							const handle = lookupSyntheticFd(oldFd >>> 0);
							if (!handle) return oldFd <= 2 && newFd <= 2 ? errnoSuccess : errnoBadf;
							const cloned = cloneSyntheticHandle(handle);
							if (!cloned) return errnoBadf;
							replaceSyntheticFd(newFd >>> 0, cloned);
							return errnoSuccess;
						},
						fd_pipe(retReadFd, retWriteFd) {
							const pipe = {
								chunks: [],
								consumers: new Map(),
								producers: new Map(),
								readHandleCount: 1,
								writeHandleCount: 1,
							};
							const readFd = allocateSyntheticFd({ kind: "pipe-read", pipe, open: true, onClose: onPipeHandleClose });
							const writeFd = allocateSyntheticFd({ kind: "pipe-write", pipe, open: true, onClose: onPipeHandleClose });
							writeU32(retReadFd, readFd);
							writeU32(retWriteFd, writeFd);
							return errnoSuccess;
						},
						proc_getpid(retPid) { return writeU32(retPid, 1); },
						proc_getppid(retPid) { return writeU32(retPid, 0); },
						proc_kill() { return errnoNosys; },
						sleep_ms(milliseconds) {
							Atomics.wait(wait, 0, 0, milliseconds >>> 0);
							return errnoSuccess;
						},
						pty_open() { return errnoNosys; },
						proc_sigaction() { return errnoSuccess; },
					},
				},
			};
			return host;
		}
		module.exports = { createWasiCommandHost };
		module.exports.default = module.exports;
	`,
	os: `
		const virtualOs = globalThis.__agentOSVirtualOs || {};
		const stringValue = (value, fallback) =>
			typeof value === "string" && value.length > 0 ? value : fallback;
		const platform = stringValue(virtualOs.platform, "linux");
		const arch = stringValue(virtualOs.arch, "x64");
		const homedir = stringValue(virtualOs.homedir, "/home/user");
		const tmpdir = stringValue(virtualOs.tmpdir, "/tmp");
		const username = stringValue(virtualOs.user, "user");
		const shell = stringValue(virtualOs.shell, "/bin/sh");
		const positiveInteger = (value, fallback) =>
			Number.isSafeInteger(value) && value > 0 ? value : fallback;
		const nonNegativeInteger = (value, fallback) =>
			Number.isSafeInteger(value) && value >= 0 ? value : fallback;
		const cpuCount = positiveInteger(virtualOs.cpuCount, 1);
		const totalmem = positiveInteger(virtualOs.totalmem, 1024 * 1024 * 1024);
		const freemem = Math.min(
			positiveInteger(virtualOs.freemem, 512 * 1024 * 1024),
			totalmem,
		);
		const uid = nonNegativeInteger(virtualOs.uid, 1000);
		const gid = nonNegativeInteger(virtualOs.gid, 1000);
		const cpuInfo = () => ({
			model: stringValue(virtualOs.cpuModel, "secure-exec virtual CPU"),
			speed: 0,
			times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 },
		});
		module.exports = {
			EOL: "\\n",
			arch: () => arch,
			cpus: () => Array.from({ length: cpuCount }, cpuInfo),
			endianness: () => "LE",
			freemem: () => freemem,
			getPriority: () => 0,
			homedir: () => homedir,
			hostname: () => stringValue(virtualOs.hostname, "secure-exec"),
			loadavg: () => [0, 0, 0],
			machine: () => stringValue(virtualOs.machine, "x86_64"),
			networkInterfaces: () => ({}),
			platform: () => platform,
			release: () => stringValue(virtualOs.release, "6.8.0-secure-exec"),
			tmpdir: () => tmpdir,
			totalmem: () => totalmem,
			type: () => stringValue(virtualOs.type, platform === "win32" ? "Windows_NT" : "Linux"),
			uptime: () => 0,
			userInfo: () => ({ username, uid, gid, shell, homedir }),
			version: () => stringValue(virtualOs.version, "#1 SMP PREEMPT_DYNAMIC secure-exec"),
		};
	`,
	"node:os": "module.exports = require('os');",
};

export function exposeCustomGlobal(name: string, value: unknown): void {
	(globalThis as Record<string, unknown>)[name] = value;
}

export function exposeMutableRuntimeStateGlobal(
	name: string,
	value: unknown,
): void {
	(globalThis as Record<string, unknown>)[name] = value;
}

export function getIsolateRuntimeSource(id: string): string {
	if (id === "overrideProcessCwd") {
		return `
			if (globalThis.process && globalThis.__runtimeProcessCwdOverride) {
				globalThis.process.cwd = () => String(globalThis.__runtimeProcessCwdOverride);
			}
		`;
	}
	return "";
}

export function getRequireSetupCode(): string {
	return `
		(function () {
			${guestEncodingBootstrapCode()}

			const callSyncBridge = (ref, ...args) => {
				if (typeof ref === "function") {
					return ref(...args);
				}
				if (ref && typeof ref.applySync === "function") {
					return ref.applySync(undefined, args);
				}
				if (ref && typeof ref.applySyncPromise === "function") {
					return ref.applySyncPromise(undefined, args);
				}
				return undefined;
			};

			const pathDirname = (value) => {
				const normalized = String(value || "/").replace(/\\\\/g, "/");
				if (normalized === "/") return "/";
				const parts = normalized.split("/").filter(Boolean);
				return parts.length <= 1 ? "/" : "/" + parts.slice(0, -1).join("/");
			};

			globalThis.require = function require(specifier) {
				const polyfillSource = callSyncBridge(
					globalThis._loadPolyfill,
					specifier.replace(/^node:/, ""),
				);
				if (polyfillSource) {
					const module = { exports: {} };
					const fn = new Function("module", "exports", polyfillSource);
					fn(module, module.exports);
					return module.exports;
				}

				const currentModule = globalThis._currentModule || { dirname: "/" };
				const resolved = callSyncBridge(
					globalThis._resolveModuleSync,
					specifier,
					currentModule.dirname || "/",
					"require",
				);
				if (!resolved) {
					throw new Error("Cannot resolve module '" + specifier + "'");
				}

				const cache = globalThis._moduleCache || (globalThis._moduleCache = {});
				if (cache[resolved]) {
					return cache[resolved].exports;
				}

				const source = callSyncBridge(
					globalThis._loadFileSync,
					resolved,
					"require",
				);
				if (source == null) {
					throw new Error("Cannot load module '" + resolved + "'");
				}

				const module = { exports: {} };
				cache[resolved] = module;
				const previous = globalThis._currentModule;
				globalThis._currentModule = { filename: resolved, dirname: pathDirname(resolved) };
				try {
					const fn = new Function(
						"require",
						"module",
						"exports",
						"__filename",
						"__dirname",
						source,
					);
					fn(globalThis.require, module, module.exports, resolved, pathDirname(resolved));
				} finally {
					globalThis._currentModule = previous;
				}
				return module.exports;
			};

			// Expose a faithful global Buffer to all guest code (npm packages
			// expect it), matching native. Backed by the real \`buffer\` package.
			if (typeof globalThis.Buffer === "undefined") {
				globalThis.Buffer = globalThis.require("buffer").Buffer;
			}

			const util = globalThis.require("util");
			const formatConsoleLine = (...args) => {
				if (!util || typeof util.formatWithOptions !== "function") {
					throw new Error("console formatting requires util.formatWithOptions");
				}
				return util.formatWithOptions({ colors: false }, ...args) + "\\n";
			};
			const writeConsole = (ref, args) => {
				callSyncBridge(ref, formatConsoleLine(...args));
			};
			const consoleObject = {
				log: (...args) => writeConsole(globalThis._log, args),
				info: (...args) => writeConsole(globalThis._log, args),
				debug: (...args) => writeConsole(globalThis._log, args),
				warn: (...args) => writeConsole(globalThis._error, args),
				error: (...args) => writeConsole(globalThis._error, args),
				dir: (value) => writeConsole(globalThis._log, [value]),
				dirxml: (...args) => writeConsole(globalThis._log, args),
				assert: (condition, ...args) => {
					if (condition) return;
					writeConsole(globalThis._error, args.length > 0 ? args : ["Assertion failed"]);
				},
				clear: () => {},
				count: (label = "default") => {
					const key = String(label);
					consoleObject._counts.set(key, (consoleObject._counts.get(key) || 0) + 1);
					consoleObject.log(key + ": " + consoleObject._counts.get(key));
				},
				countReset: (label = "default") => {
					consoleObject._counts.delete(String(label));
				},
				group: (...args) => {
					if (args.length > 0) consoleObject.log(...args);
				},
				groupCollapsed: (...args) => {
					if (args.length > 0) consoleObject.log(...args);
				},
				groupEnd: () => {},
				table: (value) => consoleObject.log(value),
				time: (label = "default") => {
					consoleObject._times.set(String(label), Date.now());
				},
				timeEnd: (label = "default") => {
					const key = String(label);
					if (!consoleObject._times.has(key)) return;
					const startedAt = consoleObject._times.get(key);
					consoleObject._times.delete(key);
					consoleObject.log(key + ": " + (Date.now() - startedAt) + "ms");
				},
				timeLog: (label = "default", ...args) => {
					const key = String(label);
					if (!consoleObject._times.has(key)) return;
					consoleObject.log(key + ": " + (Date.now() - consoleObject._times.get(key)) + "ms", ...args);
				},
				trace: (...args) => {
					const message = formatConsoleLine(...args).trimEnd();
					const error = new Error(message);
					writeConsole(globalThis._error, [error.stack || message]);
				},
				_counts: new Map(),
				_times: new Map(),
			};
			globalThis.console = consoleObject;
		})();
	`;
}

export { createInMemoryFileSystem, InMemoryFileSystem };
