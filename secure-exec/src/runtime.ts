import { AgentOs, type AgentOsOptions } from "@rivet-dev/agentos-core";
import { type Context, createContext } from "./context.js";

/** Options for the VM that runs the code: permissions, limits, mounts, and so on. */
export type VmOptions = AgentOsOptions;

// Keep in sync with `AgentOsOptions`. The record type makes a missing or
// unknown key a compile error.
const VM_OPTION_KEYS = Object.keys({
	user: true,
	software: true,
	defaultSoftware: true,
	loopbackExemptPorts: true,
	allowedNodeBuiltins: true,
	highResolutionTime: true,
	database: true,
	rootFilesystem: true,
	mounts: true,
	sandbox: true,
	scheduleDriver: true,
	hostFunctions: true,
	permissions: true,
	sidecar: true,
	limits: true,
	onAgentStderr: true,
	onAgentExit: true,
	onLimitWarning: true,
} satisfies Record<keyof VmOptions, true>) as (keyof VmOptions)[];

/**
 * A VM that lives until you dispose it. Files, installed packages, and
 * background processes persist across the calls you make on it. Every namespace
 * is the agentOS one, unchanged, so the agentOS docs describe its behavior.
 */
export interface Vm extends AsyncDisposable {
	readonly javascript: Omit<AgentOs["javascript"], "npm">;
	readonly typescript: AgentOs["typescript"];
	readonly npm: AgentOs["javascript"]["npm"];
	readonly filesystem: AgentOs["filesystem"];
	readonly network: AgentOs["network"];
	readonly process: AgentOs["process"];
	/** Create a context: JavaScript state that survives between calls. */
	createContext(): Promise<Context>;
	/** Dispose the VM and everything in it. */
	dispose(): Promise<void>;
}

export async function createVm(options: VmOptions = {}): Promise<Vm> {
	const vm = await AgentOs.create(options);
	const { npm, ...javascript } = vm.javascript;
	const dispose = () => vm.dispose();
	return {
		javascript,
		typescript: vm.typescript,
		npm,
		filesystem: vm.filesystem,
		network: vm.network,
		process: vm.process,
		createContext: () => createContext(vm),
		dispose,
		[Symbol.asyncDispose]: dispose,
	};
}

/** Options for a one-shot call: the operation's own options plus VM options. */
export type OneShot<O> = Omit<O, "contextId"> & VmOptions;

/** Start the shared sidecar process now so the first operation does not pay for it. */
export async function init(): Promise<void> {
	// A bare VM is enough to start the sidecar; skip projecting default software.
	const vm = await createVm({ defaultSoftware: false });
	await vm.dispose();
}

/**
 * Stop the shared sidecar process and every VM still running in it. Call it when
 * your process should exit while work may be in flight, such as at the end of a
 * test run. The next call starts a new sidecar.
 */
export async function shutdown(): Promise<void> {
	const sidecar = await AgentOs.getSharedSidecar();
	await sidecar.dispose();
}

/** Run `operation` in a fresh VM built from the VM options, then dispose it. */
export async function run<O extends object, R>(
	options: (O & VmOptions) | undefined,
	operation: (vm: Vm, options: O) => Promise<R>,
): Promise<R> {
	const vmOptions: Record<string, unknown> = {};
	const operationOptions: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(options ?? {})) {
		const isVmOption = (VM_OPTION_KEYS as string[]).includes(key);
		(isVmOption ? vmOptions : operationOptions)[key] = value;
	}

	const vm = await createVm(vmOptions as VmOptions);
	let result: R;
	try {
		result = await operation(vm, operationOptions as O);
	} catch (error) {
		throw await disposeAfterFailure(vm, error);
	}
	await vm.dispose();
	return result;
}

/** Dispose `vm` and return the error to throw, keeping both if disposal also fails. */
async function disposeAfterFailure(vm: Vm, error: unknown): Promise<unknown> {
	try {
		await vm.dispose();
		return error;
	} catch (disposeError) {
		return new AggregateError(
			[error, disposeError],
			"secure-exec operation and VM cleanup failed",
		);
	}
}
