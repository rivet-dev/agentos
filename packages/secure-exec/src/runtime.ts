import {
	AgentOs,
	type AgentOsOptions,
	type Permissions,
} from "@rivet-dev/agentos-core";

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
	bindings: true,
	permissions: true,
	sidecar: true,
	limits: true,
	onAgentStderr: true,
	onAgentExit: true,
	onLimitWarning: true,
} satisfies Record<keyof VmOptions, true>) as (keyof VmOptions)[];

// The documented agentOS baseline: everything virtualized is allowed, the
// network is denied, and a caller's policy is merged over it. The Rust client
// applies this itself; the TypeScript core client instead allows everything
// when `permissions` is omitted and denies every scope a partial policy leaves
// out, so apply it here until core matches.
const BASE_PERMISSIONS: Permissions = {
	fs: "allow",
	network: "deny",
	childProcess: "allow",
	process: "allow",
	env: "allow",
	binding: "allow",
};

function createVm(options: VmOptions = {}): Promise<AgentOs> {
	return AgentOs.create({
		...options,
		permissions: { ...BASE_PERMISSIONS, ...options.permissions },
	});
}

/**
 * Retained language state inside a dedicated VM. Pass it as `context` to run
 * code against that state; dispose it to release the VM.
 */
export interface Context extends AsyncDisposable {
	readonly contextId: string;
	/** Clear the retained state. The VM and its filesystem are kept. */
	reset(): Promise<void>;
	/** Dispose the VM that backs this context. */
	dispose(): Promise<void>;
}

/**
 * Where an operation runs: in an existing context, or, when `context` is
 * omitted, in a fresh VM configured by these options and disposed afterwards.
 */
export type Target =
	| ({ context: Context } & { [K in keyof VmOptions]?: never })
	| ({ context?: never } & VmOptions);

const contextVms = new WeakMap<Context, AgentOs>();

/** Start the shared sidecar process now so the first operation does not pay for it. */
export async function init(): Promise<void> {
	// A bare VM is enough to start the sidecar; skip projecting default software.
	const vm = await createVm({ defaultSoftware: false });
	await vm.dispose();
}

export async function createContext(options?: VmOptions): Promise<Context> {
	const vm = await createVm(options);
	const contextId = crypto.randomUUID();
	try {
		await vm.createContext(contextId);
	} catch (error) {
		throw await disposeAfterFailure(vm, error);
	}
	const context: Context = {
		contextId,
		reset: () => vm.contexts.reset(contextId),
		dispose: () => vm.dispose(),
		[Symbol.asyncDispose]: () => vm.dispose(),
	};
	contextVms.set(context, vm);
	return context;
}

export function contextVm(context: Context): AgentOs {
	const vm = contextVms.get(context);
	if (!vm) {
		throw new TypeError("context must be created by createContext()");
	}
	return vm;
}

/** Run `operation` in the target's context VM, or in a fresh VM disposed afterwards. */
export async function run<O extends object, R>(
	options: (O & Target) | undefined,
	operation: (vm: AgentOs, options: O & { contextId?: string }) => Promise<R>,
): Promise<R> {
	const vmOptions: Record<string, unknown> = {};
	const operationOptions: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(options ?? {})) {
		const isVmOption = (VM_OPTION_KEYS as string[]).includes(key);
		(isVmOption ? vmOptions : operationOptions)[key] = value;
	}

	const { context, ...rest } = operationOptions as { context?: Context };
	if (context) {
		const configured = Object.keys(vmOptions);
		if (configured.length > 0) {
			throw new TypeError(
				`${configured.join(", ")} cannot be combined with context; pass VM options to createContext()`,
			);
		}
		return operation(contextVm(context), {
			...(rest as O),
			contextId: context.contextId,
		});
	}

	const vm = await createVm(vmOptions as VmOptions);
	let result: R;
	try {
		result = await operation(vm, rest as O);
	} catch (error) {
		throw await disposeAfterFailure(vm, error);
	}
	await vm.dispose();
	return result;
}

/** Dispose `vm` and return the error to throw, keeping both if disposal also fails. */
async function disposeAfterFailure(
	vm: AgentOs,
	error: unknown,
): Promise<unknown> {
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
