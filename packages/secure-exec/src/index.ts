/**
 * Public Secure-Exec compatibility surface backed by Agent OS primitives.
 *
 * This intentionally exposes only the stable Node-focused API. Deferred
 * compatibility packages such as browser and Python remain out of scope.
 */

export type {
	BindingFunction,
	BindingTree,
	DefaultNetworkAdapterOptions,
	DirEntry,
	ExecOptions,
	ExecResult,
	Kernel,
	KernelInterface,
	NetworkAdapter,
	NodeRuntimeDriver,
	NodeRuntimeDriverFactory,
	NodeRuntimeDriverFactoryOptions,
	NodeRuntimeOptions,
	OSConfig,
	Permissions,
	ProcessConfig,
	ResourceBudgets,
	RunResult,
	StatInfo,
	StdioChannel,
	StdioEvent,
	StdioHook,
	TimingMitigation,
	VirtualFileSystem,
} from "@rivet-dev/agentos-core/internal/runtime-compat";
export type { NodeModulesMountConfig } from "@rivet-dev/agentos-core";
export {
	allowAll,
	allowAllChildProcess,
	allowAllEnv,
	allowAllFs,
	allowAllNetwork,
	createDefaultNetworkAdapter,
	createKernel,
	createNodeDriver,
	createNodeHostCommandExecutor,
	createNodeRuntime,
	createNodeRuntimeDriverFactory,
	exists,
	isPrivateIp,
	mkdir,
	NodeExecutionDriver,
	NodeFileSystem,
	NodeRuntime,
	readDirWithTypes,
	rename,
	stat,
} from "@rivet-dev/agentos-core/internal/runtime-compat";
export { nodeModulesMount } from "@rivet-dev/agentos-core";
