import type {
	AgentOsLimits,
	ExecOptions,
	KernelSpawnOptions,
	OpenShellOptions,
} from "../src/index.js";

// @ts-expect-error TimingMitigation was accepted but never reached the sidecar.
import type { TimingMitigation } from "../src/index.js";

const execOptions = {
	env: { MODE: "test" },
	cwd: "/workspace",
	stdin: "input",
	timeout: 100,
	onStdout: (_data: Uint8Array) => {},
	onStderr: (_data: Uint8Array) => {},
	captureStdio: false,
} satisfies ExecOptions;

const spawnOptions = {
	env: { MODE: "test" },
	cwd: "/workspace",
	timeout: 100,
	onStdout: (_data: Uint8Array) => {},
	onStderr: (_data: Uint8Array) => {},
	streamStdin: true,
} satisfies KernelSpawnOptions;

const shellOptions = {
	command: "sh",
	args: ["-l"],
	env: { MODE: "test" },
	cwd: "/workspace",
	cols: 80,
	rows: 24,
	onStderr: (_data: Uint8Array) => {},
} satisfies OpenShellOptions;

const limits = {
	jsRuntime: { capturedOutputLimitBytes: 1024 },
} satisfies AgentOsLimits;

const removedExecFilePath = {
	// @ts-expect-error filePath was never forwarded to the sidecar.
	filePath: "/workspace/entry.mjs",
} satisfies ExecOptions;

const removedExecCpuTime = {
	// @ts-expect-error per-exec CPU policy was never forwarded to the sidecar.
	cpuTimeLimitMs: 100,
} satisfies ExecOptions;

const removedExecTiming = {
	// @ts-expect-error timing mitigation was never forwarded to the sidecar.
	timingMitigation: "freeze",
} satisfies ExecOptions;

const removedSpawnStdin = {
	// @ts-expect-error spawn stdin was accepted but ignored.
	stdin: "input",
} satisfies KernelSpawnOptions;

const removedSpawnCapture = {
	// @ts-expect-error spawn captureStdio was accepted but ignored.
	captureStdio: false,
} satisfies KernelSpawnOptions;

const removedSpawnStdio = {
	// @ts-expect-error stdio inheritance was accepted but ignored.
	stdio: "inherit",
} satisfies KernelSpawnOptions;

const removedSpawnStdinFd = {
	// @ts-expect-error host file descriptors cannot be passed into the guest.
	stdinFd: 0,
} satisfies KernelSpawnOptions;

const removedSpawnStdoutFd = {
	// @ts-expect-error host file descriptors cannot be passed into the guest.
	stdoutFd: 1,
} satisfies KernelSpawnOptions;

const removedSpawnStderrFd = {
	// @ts-expect-error host file descriptors cannot be passed into the guest.
	stderrFd: 2,
} satisfies KernelSpawnOptions;

const removedSpawnPty = {
	// @ts-expect-error PTY configuration belongs to openShell, not raw spawn.
	pty: { cols: 80, rows: 24 },
} satisfies KernelSpawnOptions;

const removedStdinBufferLimit = {
	jsRuntime: {
		// @ts-expect-error this is a fixed executor implementation bound.
		stdinBufferLimitBytes: 1,
	},
} satisfies AgentOsLimits;

const removedEventPayloadLimit = {
	jsRuntime: {
		// @ts-expect-error this is a fixed executor implementation bound.
		eventPayloadLimitBytes: 1,
	},
} satisfies AgentOsLimits;

const removedIpcFrameLimit = {
	jsRuntime: {
		// @ts-expect-error this is a fixed executor implementation bound.
		v8IpcMaxFrameBytes: 1,
	},
} satisfies AgentOsLimits;

void (null as TimingMitigation | null);
void execOptions;
void spawnOptions;
void shellOptions;
void limits;
void removedExecFilePath;
void removedExecCpuTime;
void removedExecTiming;
void removedSpawnStdin;
void removedSpawnCapture;
void removedSpawnStdio;
void removedSpawnStdinFd;
void removedSpawnStdoutFd;
void removedSpawnStderrFd;
void removedSpawnPty;
void removedStdinBufferLimit;
void removedEventPayloadLimit;
void removedIpcFrameLimit;
