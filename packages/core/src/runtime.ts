export type StdioChannel = "stdout" | "stderr";
export type TimingMitigation = "off" | "freeze";
export type PermissionMode = "allow" | "deny";
export type PermissionDecision = PermissionMode;

export interface VirtualDirEntry {
	name: string;
	isDirectory: boolean;
	isSymbolicLink?: boolean;
}

export interface VirtualStat {
	mode: number;
	size: number;
	sizeExact?: bigint;
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
	inoExact?: bigint;
	nlink: number;
	nlinkExact?: bigint;
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

export interface NetworkAccessRequest {
	url?: string;
	host?: string;
	port?: number;
	protocol?: string;
}

export interface FsPermissionRule {
	mode: PermissionMode;
	operations?: string[];
	paths?: string[];
}

export interface PatternPermissionRule {
	mode: PermissionMode;
	operations?: string[];
	patterns?: string[];
}

export interface RulePermissions<TRule> {
	default?: PermissionMode;
	rules: TRule[];
}

export type FsPermissions = PermissionMode | RulePermissions<FsPermissionRule>;
export type NetworkPermissions =
	| PermissionMode
	| RulePermissions<PatternPermissionRule>;
export type ChildProcessPermissions =
	| PermissionMode
	| RulePermissions<PatternPermissionRule>;
export type ProcessPermissions =
	| PermissionMode
	| RulePermissions<PatternPermissionRule>;
export type EnvPermissions =
	| PermissionMode
	| RulePermissions<PatternPermissionRule>;
export type BindingPermissions =
	| PermissionMode
	| RulePermissions<PatternPermissionRule>;

export interface ProcessInfo {
	pid: number;
	ppid: number;
	pgid: number;
	sid: number;
	driver: string;
	command: string;
	args: string[];
	cwd: string;
	status: "running" | "stopped" | "exited";
	exitCode: number | null;
	startTime: number;
	exitTime: number | null;
}

export interface ManagedProcess {
	pid: number;
	writeStdin(data: Uint8Array | string): Promise<void>;
	closeStdin(): Promise<void>;
	kill(signal?: number): void | Promise<void>;
	wait(): Promise<number>;
	readonly exitCode: number | null;
}

export interface ShellHandle {
	pid: number;
	/** Sidecar-owned process correlation id. */
	processId: string;
	write(data: Uint8Array | string): Promise<void>;
	onData: ((data: Uint8Array) => void) | null;
	resize(cols: number, rows: number): void | Promise<void>;
	kill(signal?: number): void | Promise<void>;
	wait(): Promise<number>;
}

export interface OpenShellOptions {
	command?: string;
	args?: string[];
	env?: Record<string, string>;
	cwd?: string;
	cols?: number;
	rows?: number;
	onStderr?: (data: Uint8Array) => void;
}

export interface ExecOptions {
	env?: Record<string, string>;
	cwd?: string;
	stdin?: string | Uint8Array;
	timeout?: number;
	onStdout?: (data: Uint8Array) => void;
	onStderr?: (data: Uint8Array) => void;
	captureStdio?: boolean;
	filePath?: string;
	cpuTimeLimitMs?: number;
	timingMitigation?: TimingMitigation;
}

export interface ExecResult {
	exitCode: number;
	stdout: string;
	stderr: string;
}

export interface KernelSpawnOptions extends ExecOptions {
	stdio?: "pipe" | "inherit";
	stdinFd?: number;
	stdoutFd?: number;
	stderrFd?: number;
	streamStdin?: boolean;
	pty?: { cols?: number; rows?: number };
}

export type KernelExecOptions = ExecOptions;
export type KernelExecResult = ExecResult;
export interface Permissions {
	fs?: FsPermissions;
	network?: NetworkPermissions;
	childProcess?: ChildProcessPermissions;
	process?: ProcessPermissions;
	env?: EnvPermissions;
	binding?: BindingPermissions;
}

export interface KernelInterface {
	vfs: VirtualFileSystem;
}

export interface KernelRecursiveDirEntry {
	name: string;
	path: string;
	isDirectory: boolean;
	isSymbolicLink: boolean;
	size: number;
}

export interface Kernel extends KernelInterface {
	dispose(): Promise<void>;
	exec(command: string, options?: KernelExecOptions): Promise<KernelExecResult>;
	spawn(
		command: string,
		args: string[],
		options?: KernelSpawnOptions,
	): Promise<ManagedProcess>;
	openShell(options?: OpenShellOptions): Promise<ShellHandle>;
	mountFs(
		path: string,
		fs: VirtualFileSystem,
		options?: { readOnly?: boolean },
	): void | Promise<void>;
	unmountFs(path: string): void | Promise<void>;
	readFile(path: string): Promise<Uint8Array>;
	writeFile(path: string, content: string | Uint8Array): Promise<void>;
	mkdir(path: string, options?: { recursive?: boolean }): Promise<void>;
	readdir(path: string): Promise<string[]>;
	readdirRecursive(
		path: string,
		options?: { maxDepth?: number },
	): Promise<KernelRecursiveDirEntry[]>;
	stat(path: string): Promise<VirtualStat>;
	exists(path: string): Promise<boolean>;
	removeFile(path: string): Promise<void>;
	removeDir(path: string): Promise<void>;
	removePath(path: string, options?: { recursive?: boolean }): Promise<void>;
	rename(oldPath: string, newPath: string): Promise<void>;
	movePath(oldPath: string, newPath: string): Promise<void>;
	readonly commands: ReadonlyMap<string, string>;
	readonly processes: ReadonlyMap<number, ProcessInfo>;
	readonly env: Record<string, string>;
	readonly cwd: string;
}
