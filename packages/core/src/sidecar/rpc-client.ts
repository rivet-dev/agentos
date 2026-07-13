import { randomUUID } from "node:crypto";
import { constants as osConstants } from "node:os";
import { posix as posixPath } from "node:path";
import type {
	RootFilesystemConfig as VmConfigRootFilesystemConfig,
	RootFilesystemEntry as VmConfigRootFilesystemEntry,
	RootFilesystemLowerDescriptor as VmConfigRootFilesystemLowerDescriptor,
} from "@rivet-dev/agentos-runtime-core/vm-config";
import type {
	NativeMountConfig,
	PlainMountConfig,
	RootFilesystemConfig,
	RootLowerInput,
} from "../agent-os.js";
import type { FilesystemEntry } from "../filesystem-snapshot.js";
import type {
	KernelExecOptions,
	KernelExecResult,
	KernelSpawnOptions,
	ManagedProcess,
	OpenShellOptions,
	ProcessInfo,
	ShellHandle,
	VirtualFileSystem,
	VirtualStat,
} from "../runtime.js";
import type {
	AuthenticatedSession,
	CreatedVm,
	GuestFilesystemStat,
	SidecarProcess,
	SidecarProcessSnapshotEntry,
} from "./native-process-client.js";

function shouldLogStructuredSidecarEvent(name: string): boolean {
	const normalized = name.toLowerCase();
	return (
		normalized === "limit_warning" ||
		normalized.startsWith("security.") ||
		normalized.includes("warning") ||
		normalized.includes("failed") ||
		normalized.includes("error")
	);
}

function formatStructuredSidecarDetail(
	detail: Readonly<Record<string, string>>,
): string {
	const entries = Object.entries(detail);
	if (entries.length === 0) {
		return "";
	}
	return entries
		.map(([key, value]) => `${key}=${JSON.stringify(value)}`)
		.join(" ");
}

function logStructuredSidecarEvent(
	name: string,
	detail: Readonly<Record<string, string>>,
): void {
	if (!shouldLogStructuredSidecarEvent(name)) {
		return;
	}
	const formatted = formatStructuredSidecarDetail(detail);
	console.warn(
		formatted
			? `[agent-os] sidecar ${name}: ${formatted}`
			: `[agent-os] sidecar ${name}`,
	);
}

const PREFERRED_SIGNAL_NAMES = [
	"SIGHUP",
	"SIGINT",
	"SIGQUIT",
	"SIGILL",
	"SIGTRAP",
	"SIGABRT",
	"SIGBUS",
	"SIGFPE",
	"SIGKILL",
	"SIGUSR1",
	"SIGSEGV",
	"SIGUSR2",
	"SIGPIPE",
	"SIGALRM",
	"SIGTERM",
	"SIGSTKFLT",
	"SIGCHLD",
	"SIGCONT",
	"SIGSTOP",
	"SIGTSTP",
	"SIGTTIN",
	"SIGTTOU",
	"SIGURG",
	"SIGXCPU",
	"SIGXFSZ",
	"SIGVTALRM",
	"SIGPROF",
	"SIGWINCH",
	"SIGIO",
	"SIGPWR",
	"SIGSYS",
	"SIGEMT",
	"SIGINFO",
] as const;
const NON_CANONICAL_SIGNAL_NAMES = new Set([
	"SIGCLD",
	"SIGIOT",
	"SIGPOLL",
	"SIGUNUSED",
]);
const SIGNAL_NAME_BY_NUMBER = buildSignalNameByNumber();
function buildSignalNameByNumber(): Map<number, string> {
	const signals = osConstants.signals as Record<string, number | undefined>;
	const names = new Map<number, string>();
	for (const name of PREFERRED_SIGNAL_NAMES) {
		const value = signals[name];
		if (typeof value === "number") {
			names.set(value, name);
		}
	}
	for (const [name, value] of Object.entries(signals)) {
		if (
			typeof value === "number" &&
			!NON_CANONICAL_SIGNAL_NAMES.has(name) &&
			!names.has(value)
		) {
			names.set(value, name);
		}
	}
	return names;
}

export function toSidecarSignalName(signal: number): string {
	return SIGNAL_NAME_BY_NUMBER.get(signal) ?? String(signal);
}

function toSidecarTimeoutMs(timeout: number | undefined): number | undefined {
	if (timeout === undefined) {
		return undefined;
	}
	if (!Number.isFinite(timeout) || timeout < 0) {
		throw new RangeError(
			"process timeout must be a finite non-negative number",
		);
	}
	return Math.trunc(timeout);
}

export interface LocalCompatMount {
	path: string;
	fs: VirtualFileSystem;
	readOnly?: boolean;
	sidecarMount?: SidecarMountDescriptor;
}

interface TrackedProcessEntry {
	pid: number | null;
	processId: string | null;
	command: string | undefined;
	shellCommand: string | undefined;
	args: string[];
	requestedCwd: string | undefined;
	pty: { cols?: number; rows?: number } | undefined;
	keepStdinOpen: boolean | undefined;
	timeoutMs: number | undefined;
	env: Record<string, string> | undefined;
	exitCode: number | null;
	waitPromise: Promise<number>;
	resolveWait: (exitCode: number) => void;
	rejectWait: (error: Error) => void;
	settled: boolean;
	onStdout: Set<(data: Uint8Array) => void>;
	onStderr: Set<(data: Uint8Array) => void>;
}

interface NativeSidecarKernelProxyOptions {
	client: SidecarProcess;
	session: AuthenticatedSession;
	vm: CreatedVm;
	env: Record<string, string>;
	cwd: string;
	localMounts: LocalCompatMount[];
	sidecarMounts: SidecarMountDescriptor[];
	commandGuestPaths: ReadonlyMap<string, string>;
	onWasmCommandResolved?: (command: string) => void;
	onDispose?: () => Promise<void>;
	/**
	 * Whether this proxy owns the underlying sidecar process. When VMs share one
	 * sidecar process (the default for VMs leased from an `AgentOsSidecar`
	 * handle), each proxy tears down only its own VM on `dispose()`; the shared
	 * process is disposed when the sidecar handle is disposed. Defaults to true
	 * for the legacy one-process-per-VM path.
	 */
	ownsClient?: boolean;
}

export class NativeSidecarKernelProxy {
	readonly env: Record<string, string>;
	readonly cwd: string;
	readonly commands: ReadonlyMap<string, string>;
	readonly vfs: VirtualFileSystem;
	readonly processes = new Map<number, ProcessInfo>();

	private readonly client: SidecarProcess;
	private readonly session: AuthenticatedSession;
	private readonly vm: CreatedVm;
	private readonly ownsClient: boolean;
	private readonly localMounts: LocalCompatMount[];
	private readonly baseSidecarMounts: SidecarMountDescriptor[];
	private readonly commandDrivers: Map<string, string>;
	private readonly onWasmCommandResolved:
		| ((command: string) => void)
		| undefined;
	private readonly onDispose: (() => Promise<void>) | undefined;
	private readonly trackedProcesses = new Map<number, TrackedProcessEntry>();
	private readonly trackedProcessesById = new Map<
		string,
		TrackedProcessEntry
	>();
	private processSnapshotRefresh: Promise<
		SidecarProcessSnapshotEntry[]
	> | null = null;
	private readonly rootView: VirtualFileSystem;
	private disposed = false;
	private pumpError: Error | null = null;
	private mountReconfigurePromise: Promise<void> | null = null;
	private readonly eventPumpAbortController = new AbortController();
	private readonly eventPump: Promise<void>;

	constructor(options: NativeSidecarKernelProxyOptions) {
		this.client = options.client;
		this.session = options.session;
		this.vm = options.vm;
		this.ownsClient = options.ownsClient ?? true;
		this.env = { ...options.env };
		this.cwd = options.cwd;
		this.localMounts = [...options.localMounts].sort(
			(left, right) => right.path.length - left.path.length,
		);
		const localMountPaths = new Set(
			this.localMounts.map((mount) => mount.path),
		);
		this.baseSidecarMounts = options.sidecarMounts.filter(
			(mount) =>
				mount.plugin.id !== "js_bridge" ||
				!localMountPaths.has(posixPath.normalize(mount.guestPath)),
		);
		this.commandDrivers = buildCommandMap(options.commandGuestPaths);
		this.onWasmCommandResolved = options.onWasmCommandResolved;
		this.onDispose = options.onDispose;
		this.commands = this.commandDrivers;
		this.vfs = this.createFilesystemView();
		this.rootView = this.vfs;
		this.eventPump = this.runEventPump();
		void this.eventPump;
	}

	createRootView(): VirtualFileSystem {
		return this.rootView;
	}

	/** Resolve the host-only backing store for an exact js_bridge mount id. */
	hostFilesystemForMount(mountId: string): VirtualFileSystem | undefined {
		const normalized = posixPath.normalize(mountId);
		return this.localMounts.find((mount) => mount.path === normalized)?.fs;
	}

	registerCommandGuestPaths(
		commandGuestPaths: ReadonlyMap<string, string>,
	): void {
		for (const name of commandGuestPaths.keys()) {
			this.commandDrivers.set(name, "wasmvm");
		}
	}

	async dispose(): Promise<void> {
		if (this.disposed) {
			return;
		}
		this.disposed = true;
		this.eventPumpAbortController.abort();
		const errors: Error[] = [];
		try {
			await this.mountReconfigurePromise;
		} catch (error) {
			errors.push(toError(error));
		}

		const liveProcesses = [...this.trackedProcessesById.values()].filter(
			(entry) => !entry.settled,
		);
		const signalResults = await Promise.allSettled(
			liveProcesses.map((entry) => this.signalProcess(entry, 15)),
		);
		for (const result of signalResults) {
			if (result.status === "rejected") {
				errors.push(toError(result.reason));
			}
		}

		try {
			await this.client.disposeVm(this.session, this.vm);
		} catch (error) {
			errors.push(toError(error));
		}
		for (const entry of liveProcesses) {
			if (!entry.settled) {
				// The sidecar dispose path already performs TERM/KILL escalation for any
				// guest executions that are still live. Resolve local waiters eagerly so
				// VM teardown does not hang on killed ACP adapter processes that never
				// surface a terminal process_exited event back to the JS bridge.
				this.finishProcess(entry, 143);
			}
		}
		// Only tear down the shared sidecar process when this proxy owns it. VMs
		// leased from an `AgentOsSidecar` handle share one process, which is
		// disposed when the handle is disposed.
		if (this.ownsClient) {
			try {
				await this.client.dispose();
			} catch (error) {
				errors.push(toError(error));
			}
		}
		try {
			await this.eventPump;
		} catch (error) {
			errors.push(toError(error));
		}
		try {
			await this.onDispose?.();
		} catch (error) {
			errors.push(toError(error));
		}

		// Drop all per-VM tracking state so a disposed proxy retains nothing.
		for (const entry of this.trackedProcesses.values()) {
			entry.onStdout.clear();
			entry.onStderr.clear();
		}
		this.trackedProcesses.clear();
		this.trackedProcessesById.clear();
		this.localMounts.length = 0;
		if (errors.length > 0) {
			throw new AggregateError(errors, "failed to dispose sidecar VM");
		}
	}

	/** Test-only snapshot of the per-VM tracking collection sizes. */
	__trackingSizesForTest(): {
		trackedProcesses: number;
		trackedProcessesById: number;
		localMounts: number;
	} {
		return {
			trackedProcesses: this.trackedProcesses.size,
			trackedProcessesById: this.trackedProcessesById.size,
			localMounts: this.localMounts.length,
		};
	}

	/** Test-only handle to a tracked entry so its listener Sets can be inspected. */
	__trackedEntryForTest(
		pid: number,
	):
		| { onStdout: ReadonlySet<unknown>; onStderr: ReadonlySet<unknown> }
		| undefined {
		return this.trackedProcesses.get(pid);
	}

	async exec(
		command: string,
		options?: KernelExecOptions,
	): Promise<KernelExecResult> {
		const stdoutChunks: Uint8Array[] = [];
		const stderrChunks: Uint8Array[] = [];
		const proc = await this.spawn("sh", [], {
			...options,
			shellCommand: command,
			onStdout: (chunk) => {
				stdoutChunks.push(chunk);
				options?.onStdout?.(chunk);
			},
			onStderr: (chunk) => {
				stderrChunks.push(chunk);
				options?.onStderr?.(chunk);
			},
		} as KernelSpawnOptions & { shellCommand: string });

		if (options?.stdin !== undefined) {
			await proc.writeStdin(options.stdin);
		}
		await proc.closeStdin();

		const exitCode = await proc.wait();

		return {
			exitCode,
			stdout: Buffer.concat(
				stdoutChunks.map((chunk) => Buffer.from(chunk)),
			).toString("utf8"),
			stderr: Buffer.concat(
				stderrChunks.map((chunk) => Buffer.from(chunk)),
			).toString("utf8"),
		};
	}
	async execArgv(
		command: string,
		args: readonly string[] = [],
		options?: KernelExecOptions,
	): Promise<KernelExecResult> {
		const stdoutChunks: Uint8Array[] = [];
		const stderrChunks: Uint8Array[] = [];
		const requestedCwd = options?.cwd;
		const runAndCapture = async (
			proc: ManagedProcess,
		): Promise<KernelExecResult> => {
			if (options?.stdin !== undefined) {
				proc.writeStdin(options.stdin);
			}
			proc.closeStdin();

			const exitCode = await proc.wait();

			return {
				exitCode,
				stdout: Buffer.concat(
					stdoutChunks.map((chunk) => Buffer.from(chunk)),
				).toString("utf8"),
				stderr: Buffer.concat(
					stderrChunks.map((chunk) => Buffer.from(chunk)),
				).toString("utf8"),
			};
		};

		if (this.commands.get(command) === "wasmvm") {
			this.onWasmCommandResolved?.(command);
		}

		return runAndCapture(
			await this.spawn(command, [...args], {
				...options,
				cwd: requestedCwd,
				onStdout: (chunk) => {
					stdoutChunks.push(chunk);
					options?.onStdout?.(chunk);
				},
				onStderr: (chunk) => {
					stderrChunks.push(chunk);
					options?.onStderr?.(chunk);
				},
			}),
		);
	}

	async spawn(
		command: string | undefined,
		args: string[],
		options?: KernelSpawnOptions,
	): Promise<ManagedProcess> {
		const internalOptions = options as
			| (KernelSpawnOptions & { shellCommand?: string })
			| undefined;
		const spawnCommand = command;
		const spawnArgs = [...args];
		const shellCommand = internalOptions?.shellCommand;
		let resolveWait!: (exitCode: number) => void;
		let rejectWait!: (error: Error) => void;
		const waitPromise = new Promise<number>((resolve, reject) => {
			resolveWait = resolve;
			rejectWait = reject;
		});

		const entry: TrackedProcessEntry = {
			pid: null,
			processId: null,
			command: shellCommand === undefined ? spawnCommand : undefined,
			shellCommand,
			args: spawnArgs,
			requestedCwd: options?.cwd,
			pty: options?.pty,
			env: options?.env ? { ...options.env } : undefined,
			keepStdinOpen: options?.streamStdin,
			timeoutMs: toSidecarTimeoutMs(options?.timeout),
			exitCode: null,
			waitPromise,
			resolveWait,
			rejectWait,
			settled: false,
			onStdout: new Set(options?.onStdout ? [options.onStdout] : []),
			onStderr: new Set(options?.onStderr ? [options.onStderr] : []),
		};
		await this.startTrackedProcess(entry);
		if (entry.pid === null) {
			throw new Error("sidecar did not return a kernel pid for the process");
		}
		const pid = entry.pid;

		const proc: ManagedProcess = {
			pid,
			writeStdin: async (data) => {
				if (entry.exitCode !== null) {
					return;
				}
				await this.client.writeStdin(
					this.session,
					this.vm,
					this.processIdFor(entry),
					data,
				);
			},
			closeStdin: async () => {
				if (entry.exitCode !== null) {
					return;
				}
				await this.client.closeStdin(
					this.session,
					this.vm,
					this.processIdFor(entry),
				);
			},
			kill: (signal = 15) => this.signalProcess(entry, signal),
			wait: () => this.waitForTrackedProcess(entry),
			get exitCode() {
				return entry.exitCode;
			},
		};

		return proc;
	}

	async openShell(options?: OpenShellOptions): Promise<ShellHandle> {
		const stdoutHandlers = new Set<(data: Uint8Array) => void>();
		const stderrHandlers = new Set<(data: Uint8Array) => void>();
		let onData: ((data: Uint8Array) => void) | null = null;
		stdoutHandlers.add((data) => onData?.(data));
		if (options?.onStderr) {
			stderrHandlers.add(options.onStderr);
		}

		const proc = await this.spawn(options?.command, options?.args ?? [], {
			env: options?.env,
			cwd: options?.cwd,
			pty: { cols: options?.cols, rows: options?.rows },
			onStdout: (chunk) => {
				for (const handler of stdoutHandlers) {
					handler(chunk);
				}
			},
			onStderr: (chunk) => {
				for (const handler of stderrHandlers) {
					handler(chunk);
				}
			},
		});
		const entry = this.trackedProcesses.get(proc.pid);
		if (!entry) {
			throw new Error(`sidecar shell process ${proc.pid} is not tracked`);
		}

		return {
			pid: proc.pid,
			processId: this.processIdFor(entry),
			write(data) {
				return proc.writeStdin(data);
			},
			get onData() {
				return onData;
			},
			set onData(handler) {
				onData = handler;
			},
			resize: (cols, rows) => {
				const entry = this.trackedProcesses.get(proc.pid);
				if (!entry || entry.exitCode !== null) {
					return;
				}
				return this.client.resizePty(
					this.session,
					this.vm,
					this.processIdFor(entry),
					Math.trunc(cols),
					Math.trunc(rows),
				);
			},
			kill: (signal) => proc.kill(signal),
			wait() {
				return proc.wait();
			},
		};
	}
	readFile(path: string): Promise<Uint8Array> {
		return this.dispatchNativeRead(path);
	}

	async writeFile(path: string, content: string | Uint8Array): Promise<void> {
		await this.waitForMountReconfigure();
		await this.client.writeFile(this.session, this.vm, path, content);
	}

	async mkdir(path: string, options?: { recursive?: boolean }): Promise<void> {
		await this.waitForMountReconfigure();
		await this.client.mkdir(this.session, this.vm, path, options);
	}

	async exists(path: string): Promise<boolean> {
		return this.client.exists(this.session, this.vm, path);
	}

	async stat(path: string): Promise<VirtualStat> {
		return toVirtualStat(await this.client.stat(this.session, this.vm, path));
	}

	async readdir(path: string): Promise<string[]> {
		return this.client.readdir(this.session, this.vm, path);
	}

	async readdirRecursive(
		path: string,
		options?: { maxDepth?: number },
	): Promise<
		Array<{
			name: string;
			path: string;
			isDirectory: boolean;
			isSymbolicLink: boolean;
			size: number;
		}>
	> {
		return (
			await this.client.readdirRecursive(this.session, this.vm, path, options)
		).map((entry) => ({ ...entry, size: Number(entry.size) }));
	}

	async removeFile(path: string): Promise<void> {
		await this.client.removeFile(this.session, this.vm, path);
	}

	async removeDir(path: string): Promise<void> {
		await this.client.removeDir(this.session, this.vm, path);
	}

	async removePath(
		path: string,
		options?: { recursive?: boolean },
	): Promise<void> {
		await this.client.removePath(this.session, this.vm, path, options);
	}

	async copyPath(
		fromPath: string,
		toPath: string,
		options?: { recursive?: boolean },
	): Promise<void> {
		return this.client.copyPath(
			this.session,
			this.vm,
			fromPath,
			toPath,
			options,
		);
	}

	async rename(oldPath: string, newPath: string): Promise<void> {
		return this.client.rename(this.session, this.vm, oldPath, newPath);
	}

	async movePath(oldPath: string, newPath: string): Promise<void> {
		return this.client.movePath(this.session, this.vm, oldPath, newPath);
	}

	mountFs(
		path: string,
		driver: VirtualFileSystem,
		options?: { readOnly?: boolean; sidecarMount?: SidecarMountDescriptor },
	): Promise<void> {
		this.localMounts.unshift({
			path: posixPath.normalize(path),
			fs: driver,
			readOnly: options?.readOnly,
			sidecarMount: options?.sidecarMount,
		});
		this.localMounts.sort(
			(left, right) => right.path.length - left.path.length,
		);
		return this.reconfigureSidecarMounts();
	}

	unmountFs(path: string): Promise<void> {
		const normalized = posixPath.normalize(path);
		const index = this.localMounts.findIndex(
			(mount) => mount.path === normalized,
		);
		if (index < 0) {
			return Promise.resolve();
		}
		this.localMounts.splice(index, 1);
		return this.reconfigureSidecarMounts();
	}

	private desiredSidecarMounts(): SidecarMountDescriptor[] {
		return [
			...this.baseSidecarMounts,
			...this.localMounts.map(
				(mount) =>
					mount.sidecarMount ?? {
						guestPath: mount.path,
						readOnly: mount.readOnly,
						plugin: {
							id: "js_bridge",
						},
					},
			),
		];
	}

	private reconfigureSidecarMounts(): Promise<void> {
		const run = async () => {
			if (this.disposed) {
				return;
			}
			// Package projections are sidecar-owned and survive mount reconfiguration.
			await this.client.configureVm(this.session, this.vm, {
				mounts: this.desiredSidecarMounts(),
			});
		};
		const previous = this.mountReconfigurePromise ?? Promise.resolve();
		const next = previous.then(run, run);
		const tracked = next.finally(() => {
			if (this.mountReconfigurePromise === tracked) {
				this.mountReconfigurePromise = null;
			}
		});
		this.mountReconfigurePromise = tracked;
		return tracked;
	}

	private async waitForMountReconfigure(): Promise<void> {
		if (this.mountReconfigurePromise) {
			await this.mountReconfigurePromise;
		}
	}

	async snapshotProcesses(): Promise<ProcessInfo[]> {
		return this.buildProcessSnapshot(await this.refreshProcessSnapshot());
	}

	async processSnapshotById(
		processId: string,
	): Promise<SidecarProcessSnapshotEntry | undefined> {
		return (await this.refreshProcessSnapshot()).find(
			(process) => process.processId === processId,
		);
	}

	private async refreshProcessSnapshot(): Promise<
		SidecarProcessSnapshotEntry[]
	> {
		if (this.processSnapshotRefresh) {
			return this.processSnapshotRefresh;
		}

		this.processSnapshotRefresh = (async () => {
			try {
				return await this.client.getProcessSnapshot(this.session, this.vm);
			} finally {
				this.processSnapshotRefresh = null;
			}
		})();

		return this.processSnapshotRefresh;
	}

	private async startTrackedProcess(entry: TrackedProcessEntry): Promise<void> {
		await this.waitForMountReconfigure();
		const started = await this.client.execute(this.session, this.vm, {
			...(entry.shellCommand !== undefined
				? { shellCommand: entry.shellCommand }
				: entry.command !== undefined
					? { command: entry.command }
					: {}),
			args: entry.args,
			...(entry.env !== undefined ? { env: entry.env } : {}),
			...(entry.requestedCwd !== undefined ? { cwd: entry.requestedCwd } : {}),
			...(entry.pty ? { pty: entry.pty } : {}),
			...(entry.keepStdinOpen ? { keepStdinOpen: true } : {}),
			...(entry.timeoutMs !== undefined ? { timeoutMs: entry.timeoutMs } : {}),
		});
		if (started.pid === null) {
			throw new Error("sidecar did not return a kernel pid for the process");
		}
		entry.processId = started.processId;
		entry.pid = started.pid;
		this.trackedProcessesById.set(entry.processId, entry);
		this.trackedProcesses.set(entry.pid, entry);
	}

	private async runEventPump(): Promise<void> {
		// Scope the pump to THIS VM's ownership so multiple proxies can share one
		// sidecar process: events for other VMs stay buffered for their own pumps
		// rather than being consumed (and dropped) here.
		const vmId = this.vm.vmId;
		while (!this.disposed) {
			try {
				const event = await this.client.waitForEvent(
					(frame) =>
						frame.ownership.scope === "vm" && frame.ownership.vm_id === vmId,
					undefined,
					{ signal: this.eventPumpAbortController.signal },
				);
				if (event.payload.type === "process_output") {
					const entry = this.trackedProcessesById.get(event.payload.process_id);
					if (!entry) {
						continue;
					}
					const chunk = event.payload.chunk;
					const listeners =
						event.payload.channel === "stdout"
							? entry.onStdout
							: entry.onStderr;
					for (const listener of listeners) {
						listener(chunk);
					}
					continue;
				}

				if (event.payload.type === "process_exited") {
					const entry = this.trackedProcessesById.get(event.payload.process_id);
					if (!entry) {
						continue;
					}
					this.finishProcess(entry, event.payload.exit_code);
					continue;
				}

				if (event.payload.type === "structured") {
					logStructuredSidecarEvent(event.payload.name, event.payload.detail);
				}
			} catch (error) {
				if (this.disposed) {
					return;
				}
				this.pumpError =
					error instanceof Error ? error : new Error(String(error));
				for (const entry of this.trackedProcesses.values()) {
					if (entry.exitCode !== null) {
						continue;
					}
					const stderr = new TextEncoder().encode(
						`${this.pumpError.message}\n`,
					);
					for (const listener of entry.onStderr) {
						listener(stderr);
					}
					this.failProcess(entry, this.pumpError);
				}
				return;
			}
		}
	}

	private finishProcess(entry: TrackedProcessEntry, exitCode: number): void {
		if (entry.settled) {
			return;
		}
		entry.settled = true;
		entry.exitCode = exitCode;
		entry.resolveWait(exitCode);
		// The sidecar guarantees that all process_output events precede the terminal
		// process_exited event. Release client routing immediately after exit; the
		// exited record lives on in `processes` for listing.
		this.releaseProcessTracking(entry);
	}

	private failProcess(entry: TrackedProcessEntry, error: Error): void {
		if (entry.settled) {
			return;
		}
		entry.settled = true;
		entry.rejectWait(error);
		this.releaseProcessTracking(entry);
	}

	private releaseProcessTracking(entry: TrackedProcessEntry): void {
		if (entry.pid !== null) {
			this.trackedProcesses.delete(entry.pid);
		}
		if (entry.processId !== null) {
			this.trackedProcessesById.delete(entry.processId);
		}
		entry.onStdout.clear();
		entry.onStderr.clear();
	}

	private waitForTrackedProcess(entry: TrackedProcessEntry): Promise<number> {
		return entry.waitPromise;
	}

	private async signalProcess(
		entry: TrackedProcessEntry,
		signal: number,
	): Promise<void> {
		await this.client.killProcess(
			this.session,
			this.vm,
			this.processIdFor(entry),
			toSidecarSignalName(signal),
		);
	}

	private processIdFor(entry: TrackedProcessEntry): string {
		if (entry.processId === null) {
			throw new Error("sidecar process has not started");
		}
		return entry.processId;
	}

	private createFilesystemView(): VirtualFileSystem {
		return {
			readFile: (path) => this.readFile(path),
			readTextFile: async (path) =>
				new TextDecoder().decode(await this.readFile(path)),
			readDir: (path) => this.readdir(path),
			readDirWithTypes: async (path) => {
				const entries = await this.readdir(path);
				return Promise.all(
					entries.map(async (name) => {
						const stat = await this.client.lstat(
							this.session,
							this.vm,
							posixPath.join(path, name),
						);
						return {
							name,
							isDirectory: stat.is_directory,
							isSymbolicLink: stat.is_symbolic_link,
						};
					}),
				);
			},
			writeFile: (path, content) => this.writeFile(path, content),
			createDir: async (path) => {
				try {
					await this.client.mkdir(this.session, this.vm, path);
				} catch (error) {
					if (!isAlreadyExistsError(error)) {
						throw error;
					}
				}
			},
			mkdir: (path, options) => this.mkdir(path, options),
			exists: (path) => this.exists(path),
			stat: (path) => this.stat(path),
			removeFile: (path) => this.removeFile(path),
			removeDir: (path) => this.removeDir(path),
			rename: (oldPath, newPath) =>
				this.client.rename(this.session, this.vm, oldPath, newPath),
			realpath: (path) => this.client.realpath(this.session, this.vm, path),
			symlink: (target, linkPath) =>
				this.client.symlink(this.session, this.vm, target, linkPath),
			readlink: (path) => this.client.readLink(this.session, this.vm, path),
			lstat: async (path) =>
				toVirtualStat(await this.client.lstat(this.session, this.vm, path)),
			link: (oldPath, newPath) =>
				this.client.link(this.session, this.vm, oldPath, newPath),
			chmod: (path, mode) =>
				this.client.chmod(this.session, this.vm, path, mode),
			chown: (path, uid, gid) =>
				this.client.chown(this.session, this.vm, path, uid, gid),
			utimes: (path, atimeMs, mtimeMs) =>
				this.client.utimes(this.session, this.vm, path, atimeMs, mtimeMs),
			truncate: (path, length) =>
				this.client.truncate(this.session, this.vm, path, length),
			pread: (path, offset, length) =>
				this.client.pread(this.session, this.vm, path, offset, length),
			pwrite: (path, offset, data) =>
				this.client.pwrite(this.session, this.vm, path, offset, data),
		};
	}
	private buildProcessSnapshot(
		snapshot: SidecarProcessSnapshotEntry[],
	): ProcessInfo[] {
		const processMap = new Map<number, ProcessInfo>();

		for (const entry of snapshot) {
			processMap.set(entry.pid, {
				pid: entry.pid,
				ppid: entry.ppid,
				pgid: entry.pgid,
				sid: entry.sid,
				driver: entry.driver,
				command: entry.command,
				args: entry.args,
				cwd: entry.cwd,
				status: entry.status,
				exitCode: entry.exitCode,
				startTime: entry.startTime,
				exitTime: entry.exitTime,
			});
		}

		this.processes.clear();
		for (const process of processMap.values()) {
			this.processes.set(process.pid, process);
		}

		return [...processMap.values()].sort((left, right) => left.pid - right.pid);
	}

	private async dispatchNativeRead(path: string): Promise<Uint8Array> {
		await this.waitForMountReconfigure();
		return this.client.readFile(this.session, this.vm, path);
	}
}

function buildCommandMap(
	commandGuestPaths: ReadonlyMap<string, string>,
): Map<string, string> {
	const commands = new Map<string, string>([
		["node", "node"],
		["npm", "node"],
		["npx", "node"],
		// `python` / `python3` are served by the embedded Pyodide runtime,
		// mirroring how `node` is served by the embedded V8 runtime.
		["python", "python"],
		["python3", "python"],
	]);
	for (const name of commandGuestPaths.keys()) {
		commands.set(name, "wasmvm");
	}
	return commands;
}

function isAlreadyExistsError(error: unknown): boolean {
	if (!(error instanceof Error)) {
		return false;
	}
	const message = error.message.toLowerCase();
	return error.message.includes("EEXIST") || message.includes("file exists");
}

// VirtualStat is a numeric, Node-default-shaped view: u64 fields above
// Number.MAX_SAFE_INTEGER lose precision here, same as Node's non-bigint
// fs.stat on the host.
function toVirtualStat(stat: GuestFilesystemStat): VirtualStat {
	return {
		mode: stat.mode,
		size: Number(stat.size),
		sizeExact: stat.size,
		blocks: Number(stat.blocks),
		dev: Number(stat.dev),
		rdev: Number(stat.rdev),
		isDirectory: stat.is_directory,
		isSymbolicLink: stat.is_symbolic_link,
		atimeMs: stat.atime_ms,
		mtimeMs: stat.mtime_ms,
		ctimeMs: stat.ctime_ms,
		birthtimeMs: stat.birthtime_ms,
		ino: Number(stat.ino),
		inoExact: stat.ino,
		nlink: Number(stat.nlink),
		nlinkExact: stat.nlink,
		uid: stat.uid,
		gid: stat.gid,
	};
}

export type {
	AuthenticatedSession,
	CreatedVm,
	GuestFilesystemStat,
	RootFilesystemEntry,
	SidecarConfigureVmResult,
	SidecarEventSelector,
	SidecarLinkPackageResult,
	SidecarPermissionsPolicy,
	SidecarProjectedAgent,
	SidecarRegisteredHostCallbackDefinition,
	SidecarRequestFrame,
	SidecarResponsePayload,
	SidecarSessionState,
	SidecarSignalHandlerRegistration,
	SidecarSocketStateEntry,
	SidecarSpawnOptions,
} from "./native-process-client.js";
export {
	NativeSidecarProcessClient,
	SidecarEventBufferOverflow,
	SidecarProcess,
	SidecarProcessError,
	SidecarProcessExited,
} from "./native-process-client.js";

export type AgentOsSidecarPlacement =
	| { kind: "shared"; pool?: string }
	| { kind: "explicit"; sidecarId: string };

export type AgentOsSidecarSessionState =
	| "connecting"
	| "ready"
	| "disposing"
	| "disposed"
	| "failed";

export type AgentOsSidecarVmState =
	| "creating"
	| "ready"
	| "disposing"
	| "disposed"
	| "failed";

export interface AgentOsSidecarSessionLifecycle {
	sessionId: string;
	placement: AgentOsSidecarPlacement;
	state: AgentOsSidecarSessionState;
	createdAt: number;
	connectedAt?: number;
	disposedAt?: number;
	lastError?: string;
	vmIds: string[];
}

export interface AgentOsSidecarVmLifecycle {
	vmId: string;
	sessionId: string;
	state: AgentOsSidecarVmState;
	createdAt: number;
	readyAt?: number;
	disposedAt?: number;
	lastError?: string;
}

export interface AgentOsSidecarSessionOptions {
	placement?: AgentOsSidecarPlacement;
	signal?: AbortSignal;
}

export interface AgentOsSidecarSessionBootstrap {
	sessionId: string;
	placement: AgentOsSidecarPlacement;
	signal?: AbortSignal;
}

export interface AgentOsSidecarVmBootstrap {
	vmId: string;
	sessionId: string;
}

export interface AgentOsSidecarTransport {
	createVm?(bootstrap: AgentOsSidecarVmBootstrap): Promise<void>;
	disposeVm?(vmId: string): Promise<void>;
	dispose(): Promise<void>;
}

export interface AgentOsSidecarClientOptions {
	createSessionTransport(
		bootstrap: AgentOsSidecarSessionBootstrap,
	): Promise<AgentOsSidecarTransport>;
	createId?: () => string;
	now?: () => number;
}

interface AgentOsSidecarVmEntry {
	lifecycle: AgentOsSidecarVmLifecycle;
}

interface AgentOsSidecarSessionEntry {
	lifecycle: AgentOsSidecarSessionLifecycle;
	transport?: AgentOsSidecarTransport;
	vms: Map<string, AgentOsSidecarVmEntry>;
}

export class AgentOsSidecarVmHandle {
	constructor(
		private readonly client: AgentOsSidecarClient,
		readonly sessionId: string,
		readonly vmId: string,
	) {}

	describe(): AgentOsSidecarVmLifecycle {
		return this.client.requireVmLifecycle(this.sessionId, this.vmId);
	}

	async dispose(): Promise<void> {
		await this.client.disposeVm(this.sessionId, this.vmId);
	}
}

export class AgentOsSidecarSessionHandle {
	constructor(
		private readonly client: AgentOsSidecarClient,
		readonly sessionId: string,
	) {}

	describe(): AgentOsSidecarSessionLifecycle {
		return this.client.requireSessionLifecycle(this.sessionId);
	}

	listVms(): AgentOsSidecarVmLifecycle[] {
		return this.client.listVms(this.sessionId);
	}

	async createVm(): Promise<AgentOsSidecarVmHandle> {
		return this.client.createVm(this.sessionId);
	}

	async dispose(): Promise<void> {
		await this.client.disposeSession(this.sessionId);
	}
}

export class AgentOsSidecarClient {
	private readonly createSessionTransport: AgentOsSidecarClientOptions["createSessionTransport"];
	private readonly createId: () => string;
	private readonly now: () => number;
	private readonly sessions = new Map<string, AgentOsSidecarSessionEntry>();
	private disposed = false;

	constructor(options: AgentOsSidecarClientOptions) {
		this.createSessionTransport = options.createSessionTransport;
		this.createId = options.createId ?? randomUUID;
		this.now = options.now ?? Date.now;
	}

	async createSession(
		options: AgentOsSidecarSessionOptions = {},
	): Promise<AgentOsSidecarSessionHandle> {
		this.assertActive();

		const sessionId = this.createId();
		const placement = clonePlacement(options.placement);
		const lifecycle: AgentOsSidecarSessionLifecycle = {
			sessionId,
			placement,
			state: "connecting",
			createdAt: this.now(),
			vmIds: [],
		};
		const entry: AgentOsSidecarSessionEntry = {
			lifecycle,
			vms: new Map(),
		};
		this.sessions.set(sessionId, entry);

		try {
			entry.transport = await this.createSessionTransport({
				sessionId,
				placement: clonePlacement(placement),
				signal: options.signal,
			});
			entry.lifecycle.state = "ready";
			entry.lifecycle.connectedAt = this.now();
			return new AgentOsSidecarSessionHandle(this, sessionId);
		} catch (error) {
			entry.lifecycle.state = "failed";
			entry.lifecycle.lastError = toErrorMessage(error);
			throw toError(error);
		}
	}

	listSessions(): AgentOsSidecarSessionLifecycle[] {
		return [...this.sessions.values()].map((entry) =>
			cloneSessionLifecycle(entry.lifecycle),
		);
	}

	requireSessionLifecycle(sessionId: string): AgentOsSidecarSessionLifecycle {
		const entry = this.getSessionEntry(sessionId);
		return cloneSessionLifecycle(entry.lifecycle);
	}

	listVms(sessionId: string): AgentOsSidecarVmLifecycle[] {
		const entry = this.getSessionEntry(sessionId);
		return [...entry.vms.values()].map((vmEntry) =>
			cloneVmLifecycle(vmEntry.lifecycle),
		);
	}

	requireVmLifecycle(
		sessionId: string,
		vmId: string,
	): AgentOsSidecarVmLifecycle {
		const vmEntry = this.getVmEntry(sessionId, vmId);
		return cloneVmLifecycle(vmEntry.lifecycle);
	}

	async createVm(sessionId: string): Promise<AgentOsSidecarVmHandle> {
		this.assertActive();

		const entry = this.getSessionEntry(sessionId);
		if (entry.lifecycle.state !== "ready" || !entry.transport) {
			throw new Error(
				`Cannot create VM for sidecar session ${sessionId} while it is ${entry.lifecycle.state}`,
			);
		}

		const vmId = this.createId();
		const vmEntry: AgentOsSidecarVmEntry = {
			lifecycle: {
				vmId,
				sessionId,
				state: "creating",
				createdAt: this.now(),
			},
		};
		entry.vms.set(vmId, vmEntry);
		entry.lifecycle.vmIds = [...entry.vms.keys()];

		try {
			await entry.transport.createVm?.({
				vmId,
				sessionId,
			});
			vmEntry.lifecycle.state = "ready";
			vmEntry.lifecycle.readyAt = this.now();
			return new AgentOsSidecarVmHandle(this, sessionId, vmId);
		} catch (error) {
			vmEntry.lifecycle.state = "failed";
			vmEntry.lifecycle.lastError = toErrorMessage(error);
			throw toError(error);
		}
	}

	async disposeVm(sessionId: string, vmId: string): Promise<void> {
		const sessionEntry = this.getSessionEntry(sessionId);
		const vmEntry = this.getVmEntry(sessionId, vmId);
		await this.disposeVmEntry(sessionEntry, vmEntry);
	}

	async disposeSession(sessionId: string): Promise<void> {
		const entry = this.getSessionEntry(sessionId);
		if (
			entry.lifecycle.state === "disposed" ||
			entry.lifecycle.state === "disposing"
		) {
			return;
		}

		entry.lifecycle.state = "disposing";

		const errors: Error[] = [];
		for (const vmEntry of entry.vms.values()) {
			try {
				await this.disposeVmEntry(entry, vmEntry);
			} catch (error) {
				errors.push(toError(error));
			}
		}

		try {
			await entry.transport?.dispose();
		} catch (error) {
			errors.push(toError(error));
		}

		if (errors.length > 0) {
			entry.lifecycle.state = "failed";
			entry.lifecycle.lastError = errors
				.map((error) => error.message)
				.join("; ");
			throw new Error(entry.lifecycle.lastError);
		}

		entry.lifecycle.state = "disposed";
		entry.lifecycle.disposedAt = this.now();
	}

	async dispose(): Promise<void> {
		if (this.disposed) {
			return;
		}

		const errors: Error[] = [];
		for (const sessionId of this.sessions.keys()) {
			try {
				await this.disposeSession(sessionId);
			} catch (error) {
				errors.push(toError(error));
			}
		}

		this.disposed = true;

		if (errors.length > 0) {
			throw new Error(errors.map((error) => error.message).join("; "));
		}
	}

	private async disposeVmEntry(
		sessionEntry: AgentOsSidecarSessionEntry,
		vmEntry: AgentOsSidecarVmEntry,
	): Promise<void> {
		if (
			vmEntry.lifecycle.state === "disposed" ||
			vmEntry.lifecycle.state === "disposing"
		) {
			return;
		}

		vmEntry.lifecycle.state = "disposing";
		try {
			await sessionEntry.transport?.disposeVm?.(vmEntry.lifecycle.vmId);
			vmEntry.lifecycle.state = "disposed";
			vmEntry.lifecycle.disposedAt = this.now();
		} catch (error) {
			vmEntry.lifecycle.state = "failed";
			vmEntry.lifecycle.lastError = toErrorMessage(error);
			throw toError(error);
		}
	}

	private getSessionEntry(sessionId: string): AgentOsSidecarSessionEntry {
		const entry = this.sessions.get(sessionId);
		if (!entry) {
			throw new Error(`Unknown sidecar session: ${sessionId}`);
		}
		return entry;
	}

	private getVmEntry(sessionId: string, vmId: string): AgentOsSidecarVmEntry {
		const entry = this.getSessionEntry(sessionId);
		const vmEntry = entry.vms.get(vmId);
		if (!vmEntry) {
			throw new Error(`Unknown sidecar VM ${vmId} for session ${sessionId}`);
		}
		return vmEntry;
	}

	private assertActive(): void {
		if (this.disposed) {
			throw new Error("Agent OS sidecar client has already been disposed");
		}
	}
}

export function createAgentOsSidecarClient(
	options: AgentOsSidecarClientOptions,
): AgentOsSidecarClient {
	return new AgentOsSidecarClient(options);
}

export type MountConfigJsonValue =
	| string
	| number
	| boolean
	| null
	| MountConfigJsonObject
	| MountConfigJsonValue[];

export interface MountConfigJsonObject {
	[key: string]: MountConfigJsonValue | undefined;
}

export interface SidecarMountPluginDescriptor {
	id: string;
	config?: MountConfigJsonObject;
}

export interface SidecarMountDescriptor {
	guestPath: string;
	readOnly?: boolean;
	plugin: SidecarMountPluginDescriptor;
}

export function serializeMountConfigForSidecar(
	mount: PlainMountConfig | NativeMountConfig,
): SidecarMountDescriptor {
	if ("driver" in mount) {
		return {
			guestPath: mount.path,
			...(mount.readOnly === undefined ? {} : { readOnly: mount.readOnly }),
			plugin: {
				id: "js_bridge",
			},
		};
	}

	return {
		guestPath: mount.path,
		...(mount.readOnly === undefined ? {} : { readOnly: mount.readOnly }),
		plugin: {
			id: mount.plugin.id,
			...(mount.plugin.config === undefined
				? {}
				: { config: mount.plugin.config }),
		},
	};
}

export type SidecarRootFilesystemDescriptor = VmConfigRootFilesystemConfig;
export type SidecarRootFilesystemLowerDescriptor =
	VmConfigRootFilesystemLowerDescriptor;
export type SidecarRootFilesystemEntry = VmConfigRootFilesystemEntry;

export function serializeRootFilesystemForSidecar(
	config?: RootFilesystemConfig,
): SidecarRootFilesystemDescriptor {
	return {
		...(config?.mode !== undefined ? { mode: config.mode } : {}),
		...(config?.disableDefaultBaseLayer !== undefined
			? { disableDefaultBaseLayer: config.disableDefaultBaseLayer }
			: {}),
		...(config?.lowers !== undefined
			? { lowers: config.lowers.map(serializeRootLowerForSidecar) }
			: {}),
	};
}

function clonePlacement(
	placement: AgentOsSidecarPlacement | undefined,
): AgentOsSidecarPlacement {
	if (!placement || placement.kind === "shared") {
		return {
			kind: "shared",
			...(placement?.pool ? { pool: placement.pool } : {}),
		};
	}

	return {
		kind: "explicit",
		sidecarId: placement.sidecarId,
	};
}

function cloneSessionLifecycle(
	lifecycle: AgentOsSidecarSessionLifecycle,
): AgentOsSidecarSessionLifecycle {
	return {
		...lifecycle,
		placement: clonePlacement(lifecycle.placement),
		vmIds: [...lifecycle.vmIds],
	};
}

function cloneVmLifecycle(
	lifecycle: AgentOsSidecarVmLifecycle,
): AgentOsSidecarVmLifecycle {
	return {
		...lifecycle,
	};
}

function serializeRootLowerForSidecar(
	lower: RootLowerInput,
): SidecarRootFilesystemLowerDescriptor {
	if (lower.kind === "bundled-base-filesystem") {
		return {
			kind: "bundledBaseFilesystem",
		};
	}

	return {
		kind: "snapshot",
		entries: lower.source.filesystem.entries.map(
			serializeFilesystemEntryForSidecar,
		),
	};
}

function serializeFilesystemEntryForSidecar(
	entry: FilesystemEntry,
): SidecarRootFilesystemEntry {
	const mode = Number.parseInt(entry.mode, 8);
	return {
		path: entry.path,
		kind: entry.type,
		mode,
		uid: entry.uid,
		gid: entry.gid,
		content: entry.content,
		encoding: entry.encoding,
		target: entry.target,
		executable: entry.type === "file" && (mode & 0o111) !== 0,
	};
}

function toError(error: unknown): Error {
	return error instanceof Error ? error : new Error(String(error));
}

function toErrorMessage(error: unknown): string {
	return toError(error).message;
}
