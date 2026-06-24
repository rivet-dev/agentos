import { execFileSync, spawn as spawnChildProcess } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
	existsSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	statSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import {
	dirname,
	join,
	posix as posixPath,
	resolve as resolveHostPath,
} from "node:path";
import { fileURLToPath } from "node:url";
import commonSoftware from "@agentos-software/common";
import type {
	MountConfigJsonObject,
	MountConfigJsonValue,
	NativeMountPluginDescriptor,
} from "@secure-exec/core/descriptors";
import type { CreateVmConfig } from "@secure-exec/core/vm-config";
import type {
	AgentCapabilities,
	AgentInfo,
	PermissionReply,
	PermissionRequest,
	PermissionRequestHandler,
	SessionConfigOption,
	SessionEventHandler,
	SessionInitData,
	SessionModeState,
} from "./agent-session-types.js";
import { type HostTool, type ToolKit, validateToolkits } from "./host-tools.js";
import { zodToJsonSchema } from "./host-tools-zod.js";
import type {
	JsonRpcNotification,
	JsonRpcRequest,
	JsonRpcResponse,
} from "./json-rpc.js";
import { parseAgentOsOptions } from "./options-schema.js";
import type {
	ConnectTerminalOptions,
	Kernel,
	KernelExecOptions,
	KernelExecResult,
	ProcessInfo as KernelProcessInfo,
	KernelSpawnOptions,
	ManagedProcess,
	OpenShellOptions,
	Permissions,
	ShellHandle,
	VirtualFileSystem,
	VirtualStat,
} from "./runtime-compat.js";
import { resolvePublishedSidecarBinary } from "./sidecar/binary.js";
import { findCargoBinary, resolveCargoBinary } from "./sidecar/cargo.js";

export type {
	MountConfigJsonObject,
	MountConfigJsonPrimitive,
	MountConfigJsonValue,
	NativeMountPluginDescriptor,
} from "@secure-exec/core/descriptors";
export type {
	AgentCapabilities,
	AgentInfo,
	PermissionReply,
	PermissionRequest,
	PermissionRequestHandler,
	SessionConfigOption,
	SessionEventHandler,
	SessionInitData,
	SessionMode,
	SessionModeState,
} from "./agent-session-types.js";
export type {
	AcpTimeoutErrorData,
	JsonRpcError,
	JsonRpcErrorData,
	JsonRpcNotification,
	JsonRpcRequest,
	JsonRpcResponse,
} from "./json-rpc.js";
export { isAcpTimeoutErrorData } from "./json-rpc.js";
export type { ConnectTerminalOptions } from "./runtime-compat.js";

const ACP_PROTOCOL_VERSION = 1;
const ACP_EXTENSION_NAMESPACE = "dev.rivet.agent-os.acp";
const SHELL_DISPOSE_TIMEOUT_MS = 5_000;
/**
 * Reserved `env` key on `AcpResumeSessionRequest` carrying the resolved adapter
 * bin entrypoint. The resume wire request omits a dedicated `adapterEntrypoint`
 * field; the sidecar reads the entrypoint from this key and strips it before
 * launching the adapter. Must stay in sync with the sidecar constant of the same
 * name in `crates/agentos-sidecar/src/acp_extension.rs`.
 */
const RESUME_ADAPTER_ENTRYPOINT_ENV = "AGENT_OS_RESUME_ADAPTER_ENTRYPOINT";

function defaultAcpClientCapabilities(): Record<string, unknown> {
	return {
		fs: {
			readTextFile: true,
			writeTextFile: true,
		},
		terminal: true,
	};
}

async function waitForTrackedExitPromises(
	promises: Promise<unknown>[],
	timeoutMs: number,
): Promise<void> {
	if (promises.length === 0) {
		return;
	}
	await Promise.race([
		Promise.allSettled(promises).then(() => undefined),
		new Promise<void>((resolve) => {
			setTimeout(resolve, timeoutMs);
		}),
	]);
}

/** Process tree node: extends kernel ProcessInfo with child references. */
export interface ProcessTreeNode extends KernelProcessInfo {
	children: ProcessTreeNode[];
}

/** A directory entry with metadata. */
export interface DirEntry {
	/** Absolute path to the entry. */
	path: string;
	type: "file" | "directory" | "symlink";
	size: number;
}

/** Options for readdirRecursive(). */
export interface ReaddirRecursiveOptions {
	/** Maximum depth to recurse (0 = only immediate children). */
	maxDepth?: number;
	/** Directory names to skip. */
	exclude?: string[];
}

/** Entry for batch write operations. */
export interface BatchWriteEntry {
	path: string;
	content: string | Uint8Array;
}

/** Result of a single file in a batch write. */
export interface BatchWriteResult {
	path: string;
	success: boolean;
	error?: string;
}

/** Result of a single file in a batch read. */
export interface BatchReadResult {
	path: string;
	content: Uint8Array | null;
	error?: string;
}

/** Entry in the agent registry, describing an available agent type. */
export interface AgentRegistryEntry {
	id: string;
	acpAdapter: string;
	agentPackage: string;
	installed: boolean;
}

import { AGENT_CONFIGS, type AgentConfig, type AgentType } from "./agents.js";
import {
	type BaseFilesystemEntry,
	getBaseEnvironment,
	getBaseFilesystemEntries,
} from "./base-filesystem.js";
import { CronManager } from "./cron/cron-manager.js";
import type { ScheduleDriver } from "./cron/schedule-driver.js";
import { TimerScheduleDriver } from "./cron/timer-driver.js";
import type {
	CronEvent,
	CronEventHandler,
	CronJob,
	CronJobInfo,
	CronJobOptions,
} from "./cron/types.js";
import {
	type FilesystemEntry,
	snapshotVirtualFilesystem,
	sortFilesystemEntries,
} from "./filesystem-snapshot.js";
import { createHostDirBackend } from "./host-dir-mount.js";
import {
	type LocalCompatMount,
	serializeMountConfigForSidecar,
} from "./js-bridge.js";
import {
	createSnapshotExport,
	type LayerStore,
	type OverlayFilesystemMode,
	type RootSnapshotExport,
	type SnapshotLayerHandle,
} from "./layers.js";
import {
	type CommandPackageMetadata,
	processSoftware,
	resolveAgentSnapshotBundle,
	resolvePackageDir,
	type SoftwareInput,
	type SoftwareRoot,
} from "./packages.js";
import type { PermissionTier } from "./runtime.js";
import { allowAll, createNodeHostNetworkAdapter } from "./runtime-compat.js";
import {
	type AcpRequest,
	type AcpResponse,
	AcpRuntimeKind,
	decodeAcpCallback,
	decodeAcpEvent,
	decodeAcpResponse,
	encodeAcpCallbackResponse,
	encodeAcpRequest,
} from "./sidecar/agentos-protocol.js";
import { serializePermissionsForSidecar } from "./sidecar/permissions.js";
import {
	type AgentOsSidecarClient,
	type AgentOsSidecarPlacement,
	type AgentOsSidecarSessionBootstrap,
	type AgentOsSidecarSessionHandle,
	type AgentOsSidecarTransport,
	type AgentOsSidecarVmBootstrap,
	type AgentOsSidecarVmHandle,
	type AuthenticatedSession,
	type CreatedVm,
	createAgentOsSidecarClient,
	NATIVE_SIDECAR_FRAME_TIMEOUT_MS,
	NativeSidecarKernelProxy,
	type RootFilesystemEntry,
	type SidecarMountDescriptor,
	type SidecarPermissionsPolicy,
	SidecarProcess,
	type SidecarRegisteredHostCallbackDefinition,
	type SidecarRequestFrame,
	type SidecarResponsePayload,
	type SidecarSessionState,
	serializeRootFilesystemForSidecar,
} from "./sidecar/rpc-client.js";

export interface AgentOsSharedSidecarOptions {
	pool?: string;
}

export interface AgentOsCreateSidecarOptions {
	sidecarId?: string;
}

export type AgentOsSidecarConfig =
	| { kind: "shared"; pool?: string }
	| { kind: "explicit"; handle: AgentOsSidecar };

export interface AgentOsSidecarDescription {
	sidecarId: string;
	placement: AgentOsSidecarPlacement;
	state: "ready" | "disposing" | "disposed";
	activeVmCount: number;
}

interface InProcessSidecarVmAdmin {
	dispose(): Promise<void>;
}

interface AgentOsSidecarVmLease<TVmAdmin extends InProcessSidecarVmAdmin> {
	sidecar: AgentOsSidecar;
	session: AgentOsSidecarSessionHandle;
	vm: AgentOsSidecarVmHandle;
	admin: TVmAdmin;
	dispose(): Promise<void>;
}

interface HostMountInfo {
	vmPath: string;
	hostPath: string;
	readOnly: boolean;
}

interface AgentOsVmAdmin extends InProcessSidecarVmAdmin {
	kernel: Kernel;
	rootView: VirtualFileSystem;
	hostMounts: HostMountInfo[];
	env: Record<string, string>;
	permissions: Permissions;
	sidecarMounts: SidecarMountDescriptor[];
	sidecarPermissions: SidecarPermissionsPolicy | undefined;
	commandPermissions: Record<string, PermissionTier>;
	loopbackExemptPorts: number[] | undefined;
	sidecarClient: SidecarProcess;
	sidecarSession: AuthenticatedSession;
	sidecarVm: CreatedVm;
	snapshotRootFilesystem?: () => Promise<RootSnapshotExport>;
	toolKits: ToolKit[];
	toolReference: string;
}

interface SessionEventSubscriber {
	handler: SessionEventHandler;
}

interface AgentSessionEntry {
	sessionId: string;
	agentType: string;
	processId: string;
	pid: number | null;
	closed: boolean;
	modes: SessionModeState | null;
	configOptions: SessionConfigOption[];
	capabilities: AgentCapabilities;
	agentInfo: AgentInfo | null;
	eventHandlers: Set<SessionEventSubscriber>;
	permissionHandlers: Set<PermissionRequestHandler>;
	configOverrides: Map<string, string>;
	pendingPermissionReplies: Map<
		string,
		{
			resolve: (reply: PermissionReply) => void;
			reject: (error: Error) => void;
			timer: ReturnType<typeof setTimeout>;
		}
	>;
}

interface AcpTerminalEntry {
	handle: ShellHandle;
	output: string;
	truncated: boolean;
	outputByteLimit: number;
	exitCode: number | null;
	waitPromise: Promise<number>;
}

interface ShellEntry {
	handle: ShellHandle;
	dataHandlers: Set<(data: Uint8Array) => void>;
	exitPromise: Promise<void>;
}

export type RootLowerInput =
	| { kind: "bundled-base-filesystem" }
	| RootSnapshotExport;

export interface RootFilesystemConfig {
	type?: "overlay";
	mode?: OverlayFilesystemMode;
	disableDefaultBaseLayer?: boolean;
	lowers?: RootLowerInput[];
}

/**
 * Compatibility path for arbitrary caller-supplied filesystems.
 * This maps to the sidecar `js_bridge` plugin during the migration.
 */
export interface PlainMountConfig {
	/** Path inside the VM to mount at. */
	path: string;
	/** The filesystem driver to mount. */
	driver: VirtualFileSystem;
	/** If true, write operations throw EROFS. */
	readOnly?: boolean;
}

/** Declarative native mount configuration that the sidecar can serialize. */
export interface NativeMountConfig {
	path: string;
	plugin: NativeMountPluginDescriptor;
	readOnly?: boolean;
}

export interface OverlayMountConfig {
	path: string;
	filesystem: {
		type: "overlay";
		store: LayerStore;
		mode?: OverlayFilesystemMode;
		lowers: SnapshotLayerHandle[];
	};
}

export type MountConfig =
	| PlainMountConfig
	| NativeMountConfig
	| OverlayMountConfig;

/**
 * Operator-tunable runtime limits for a VM. Every field is optional; unset fields fall back to
 * built-in defaults that match the runtime's historical hardcoded constants, so behavior is
 * unchanged unless a value is overridden. All values are JSON-serializable integers and are
 * forwarded to the native sidecar in the typed create-VM JSON config. Unknown, negative, or
 * non-integer values are rejected by the sidecar before VM construction.
 */
export interface AgentOsLimits {
	/** Kernel resource limits (processes, FDs, sockets, filesystem bytes, WASM caps, etc.). */
	resources?: {
		cpuCount?: number;
		maxProcesses?: number;
		maxOpenFds?: number;
		maxPipes?: number;
		maxPtys?: number;
		maxSockets?: number;
		maxConnections?: number;
		maxSocketBufferedBytes?: number;
		maxSocketDatagramQueueLen?: number;
		maxFilesystemBytes?: number;
		maxInodeCount?: number;
		maxBlockingReadMs?: number;
		maxPreadBytes?: number;
		maxFdWriteBytes?: number;
		maxProcessArgvBytes?: number;
		maxProcessEnvBytes?: number;
		maxReaddirEntries?: number;
		maxWasmFuel?: number;
		maxWasmMemoryBytes?: number;
		maxWasmStackBytes?: number;
	};
	/** HTTP body buffering limits. */
	http?: {
		/** Cap on `vm.fetch()` buffered response bodies. Must be <= the sidecar wire frame cap. */
		maxFetchResponseBytes?: number;
	};
	/** Host-tool registration and invocation limits. */
	tools?: {
		defaultToolTimeoutMs?: number;
		maxToolTimeoutMs?: number;
		maxRegisteredToolkits?: number;
		maxRegisteredToolsPerVm?: number;
		maxToolsPerToolkit?: number;
		maxToolSchemaBytes?: number;
		maxToolExamplesPerTool?: number;
		maxToolExampleInputBytes?: number;
	};
	/** Mount plugin manifest size limits. */
	plugins?: {
		maxPersistedManifestBytes?: number;
		maxPersistedManifestFileBytes?: number;
	};
	/** ACP adapter buffering limits. */
	acp?: {
		maxReadLineBytes?: number;
		stdoutBufferByteLimit?: number;
	};
	/** Guest JavaScript runtime buffering limits. */
	jsRuntime?: {
		v8HeapLimitMb?: number;
		capturedOutputLimitBytes?: number;
		stdinBufferLimitBytes?: number;
		eventPayloadLimitBytes?: number;
		v8IpcMaxFrameBytes?: number;
	};
	/** Guest Python runtime limits. */
	python?: {
		outputBufferMaxBytes?: number;
		executionTimeoutMs?: number;
		vfsRpcTimeoutMs?: number;
	};
	/** Guest WASM runtime limits. */
	wasm?: {
		maxModuleFileBytes?: number;
		capturedOutputLimitBytes?: number;
		syncReadLimitBytes?: number;
	};
}

export interface AgentStderrEvent {
	sessionId: string;
	agentType: string;
	processId: string;
	pid: number | null;
	chunk: Uint8Array;
}

export type AgentStderrHandler = (event: AgentStderrEvent) => void;

function defaultAgentStderrHandler(event: AgentStderrEvent): void {
	process.stderr.write(event.chunk);
}

/**
 * A near-capacity warning for one bounded limit (a queue/buffer, a saturating
 * resource cap, or a memory envelope) inside the VM runtime. Delivered the moment
 * usage crosses the runtime's warning threshold (~80%), once per crossing — the
 * runtime applies edge-triggering + hysteresis, so this never spams.
 */
export interface LimitWarning {
	/** Stable limit name, e.g. `"javascript_event_channel"` or `"vm_open_fds"`. */
	limit: string;
	/** Limit class: `"queue"`, `"resource"`, or `"memory"`. */
	category: string;
	/** Current observed usage. */
	observed: number;
	/** Configured capacity. */
	capacity: number;
	/** Observed fill as a percentage of capacity (0–100). */
	fillPercent: number;
}

export type LimitWarningHandler = (warning: LimitWarning) => void;

/**
 * Public core VM options.
 *
 * Keep this interface in sync with
 * `packages/core/src/options-schema.ts::agentOsOptionsSchema`. The Rivet
 * native actor intentionally accepts only a subset via
 * `packages/agentos/src/config.ts::nativeAgentOsOptionsSchema`.
 */
export interface AgentOsOptions {
	/**
	 * Software to install in the VM. Each entry provides agents, tools,
	 * or WASM commands. Any object with a `commandDir` property (e.g.,
	 * registry packages like @agentos-software/coreutils) is treated
	 * as a WASM command source automatically. Arrays are flattened, so
	 * meta-packages that export arrays of sub-packages work directly.
	 */
	software?: SoftwareInput[];
	/**
	 * Whether to auto-include the default software bundle (`@agentos-software/common`
	 * — `sh` + coreutils + the standard CLI tools agents rely on) in addition to
	 * any `software` you pass. Defaults to `true`; set `false` for a bare VM with
	 * only the software you list explicitly. Entries already present in `software`
	 * are not duplicated.
	 */
	defaultSoftware?: boolean;
	/** Loopback ports to exempt from SSRF checks (for testing with host-side mock servers). */
	loopbackExemptPorts?: number[];
	/**
	 * Allowed Node.js builtins for guest Node processes.
	 * Defaults to the hardened builtin set used by the native sidecar bridge.
	 */
	allowedNodeBuiltins?: string[];
	/** Root filesystem configuration. Defaults to an overlay with the bundled base snapshot as its deepest lower. */
	rootFilesystem?: RootFilesystemConfig;
	/** Filesystems to mount at boot time. */
	mounts?: MountConfig[];
	/**
	 * @deprecated Use `mounts: [nodeModulesMount(path)]` instead.
	 * Compatibility alias for mounting `<moduleAccessCwd>/node_modules` at `/root/node_modules`.
	 */
	moduleAccessCwd?: string;
	/** Additional instructions appended to the base OS system prompt injected at session start. */
	additionalInstructions?: string;
	/** Custom schedule driver for cron jobs. Defaults to TimerScheduleDriver. */
	scheduleDriver?: ScheduleDriver;
	/** Host-side toolkits available to agents inside the VM. */
	toolKits?: ToolKit[];
	/**
	 * Custom permission policy for the kernel. Controls access to filesystem,
	 * network, child process, and environment operations. Defaults to allowAll.
	 */
	permissions?: Permissions;
	/**
	 * Sidecar placement for the VM. Defaults to the shared `default` pool.
	 * Pass an explicit sidecar handle to pin the VM to a caller-managed sidecar.
	 */
	sidecar?: AgentOsSidecarConfig;
	/**
	 * Operator-tunable runtime limits. Unset fields use built-in defaults that match the
	 * runtime's historical constants, so omitting this leaves behavior unchanged.
	 */
	limits?: AgentOsLimits;
	/**
	 * Called with stderr chunks from the top-level ACP-speaking agent process.
	 * The agent process uses stdout for ACP JSON-RPC protocol traffic, so only
	 * stderr is forwarded through this hook. Defaults to writing chunks to
	 * `process.stderr`.
	 */
	onAgentStderr?: AgentStderrHandler;
	/**
	 * Called when a bounded limit inside the VM runtime approaches capacity
	 * (~80%, edge-triggered with hysteresis so it does not spam). Use it to alert
	 * on a slow consumer or a runaway guest before the limit is actually hit.
	 */
	onLimitWarning?: LimitWarningHandler;
}

/** Configuration for a local MCP server (spawned as a child process). */
export interface McpServerConfigLocal {
	type: "local";
	/** Command to launch the MCP server. */
	command: string;
	/** Arguments for the command. */
	args?: string[];
	/** Environment variables for the server process. */
	env?: Record<string, string>;
}

/** Configuration for a remote MCP server (connected via URL). */
export interface McpServerConfigRemote {
	type: "remote";
	/** URL of the remote MCP server. */
	url: string;
	/** HTTP headers to include in requests to the server. */
	headers?: Record<string, string>;
}

export type McpServerConfig = McpServerConfigLocal | McpServerConfigRemote;

export interface AgentOsRuntimeAdmin {
	kernel: Kernel;
	rootView: VirtualFileSystem;
	env: Record<string, string>;
	sidecar: AgentOsSidecar;
}

export interface CreateSessionOptions {
	/** Working directory for the agent session inside the VM. */
	cwd?: string;
	/** Environment variables to pass to the agent process. */
	env?: Record<string, string>;
	/** MCP servers to make available to the agent during the session. */
	mcpServers?: McpServerConfig[];
	/** Skip OS instructions injection entirely (default false). */
	skipOsInstructions?: boolean;
	/** Additional instructions appended to the base OS instructions. */
	additionalInstructions?: string;
}

/**
 * Options for {@link AgentOs.resumeSession}.
 *
 * Resume depends on a durable root: after a Rivet actor sleeps (VM destroyed) and
 * wakes (fresh VM, actor SQLite intact) the caller can keep prompting an existing
 * session. On a non-durable (default in-memory) root there is no surviving store,
 * so the sidecar's universal fallback tier always runs and the transcript pointer
 * is the only continuity mechanism.
 */
export interface ResumeSessionOptions {
	/**
	 * Guest-readable path to the reconstructed transcript. When present, the
	 * fallback tier arms a continuation preamble pointing the agent at it.
	 */
	transcriptPath?: string;
	/** Working directory for the resumed agent session (default `/workspace`). */
	cwd?: string;
	/** Environment variables to pass to the resumed agent process. */
	env?: Record<string, string>;
}

/** Result from {@link AgentOs.resumeSession}. */
export interface ResumeSessionResult {
	/**
	 * The live ACP session id in the fresh VM: equal to the requested id for
	 * native loads, or a freshly assigned id for the fallback tier — the caller
	 * remaps `external -> live`.
	 */
	sessionId: string;
	/** `"native"` (session/load|resume) or `"fallback"` (session/new + preamble). */
	mode: string;
}

export interface SessionInfo {
	sessionId: string;
	agentType: string;
}

/** Result from AgentOs.prompt(). */
export interface PromptResult {
	/** Raw JSON-RPC response from the ACP adapter. */
	response: JsonRpcResponse;
	/** Accumulated agent text output from streamed message chunks. */
	text: string;
}

/** Information about a process spawned via AgentOs.spawn(). */
export interface SpawnedProcessInfo {
	pid: number;
	command: string;
	args: string[];
	running: boolean;
	exitCode: number | null;
}

const LEGACY_PERMISSION_METHOD = "request/permission";
const ACP_PERMISSION_METHOD = "session/request_permission";

class AcpDispatchError extends Error {
	readonly code: number;
	readonly data?: Record<string, unknown>;

	constructor(code: number, message: string, data?: Record<string, unknown>) {
		super(message);
		this.name = "AcpDispatchError";
		this.code = code;
		this.data = data;
	}
}

function toJsonRpcNotification(value: unknown): JsonRpcNotification {
	if (
		!value ||
		typeof value !== "object" ||
		Array.isArray(value) ||
		(value as { jsonrpc?: unknown }).jsonrpc !== "2.0" ||
		typeof (value as { method?: unknown }).method !== "string"
	) {
		throw new Error("Invalid JSON-RPC notification from sidecar");
	}
	return value as JsonRpcNotification;
}

function toJsonRpcResponse(value: unknown): JsonRpcResponse {
	if (
		!value ||
		typeof value !== "object" ||
		Array.isArray(value) ||
		(value as { jsonrpc?: unknown }).jsonrpc !== "2.0" ||
		!(
			typeof (value as { id?: unknown }).id === "number" ||
			typeof (value as { id?: unknown }).id === "string" ||
			(value as { id?: unknown }).id === null
		)
	) {
		throw new Error("Invalid JSON-RPC response from sidecar");
	}
	return value as JsonRpcResponse;
}

function toJsonRpcRequest(value: unknown): JsonRpcRequest {
	if (
		!value ||
		typeof value !== "object" ||
		Array.isArray(value) ||
		(value as { jsonrpc?: unknown }).jsonrpc !== "2.0" ||
		!(
			typeof (value as { id?: unknown }).id === "number" ||
			typeof (value as { id?: unknown }).id === "string" ||
			(value as { id?: unknown }).id === null
		) ||
		typeof (value as { method?: unknown }).method !== "string"
	) {
		throw new Error("Invalid JSON-RPC request from ACP callback");
	}
	return value as JsonRpcRequest;
}

function toRecord(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: {};
}

type AcpResponseValue<TTag extends AcpResponse["tag"]> = Extract<
	AcpResponse,
	{ tag: TTag }
>["val"];

function parseAcpJson(value: string | null, context: string): unknown {
	if (value === null) {
		return undefined;
	}
	try {
		return JSON.parse(value);
	} catch (error) {
		throw new Error(
			`invalid ACP ${context} JSON: ${
				error instanceof Error ? error.message : String(error)
			}`,
		);
	}
}

function parseAcpJsonList(
	values: readonly string[],
	context: string,
): unknown[] {
	return values.map((value, index) =>
		parseAcpJson(value, `${context}[${index}]`),
	);
}

function sidecarSessionCreatedFromAcp(
	response: AcpResponseValue<"AcpSessionCreatedResponse">,
) {
	return {
		sessionId: response.sessionId,
		...(response.pid !== null ? { pid: response.pid } : {}),
		modes: parseAcpJson(response.modes, "modes"),
		configOptions: parseAcpJsonList(response.configOptions, "configOptions"),
		agentCapabilities: parseAcpJson(
			response.agentCapabilities,
			"agentCapabilities",
		),
		agentInfo: parseAcpJson(response.agentInfo, "agentInfo"),
	};
}

function sidecarSessionStateFromAcp(
	response: AcpResponseValue<"AcpSessionStateResponse">,
): SidecarSessionState {
	return {
		sessionId: response.sessionId,
		agentType: response.agentType,
		processId: response.processId,
		...(response.pid !== null ? { pid: response.pid } : {}),
		closed: response.closed,
		modes: parseAcpJson(response.modes, "modes"),
		configOptions: parseAcpJsonList(response.configOptions, "configOptions"),
		agentCapabilities: parseAcpJson(
			response.agentCapabilities,
			"agentCapabilities",
		),
		agentInfo: parseAcpJson(response.agentInfo, "agentInfo"),
	};
}

function isLocalCancelledPromptResponse(
	method: string,
	response: JsonRpcResponse,
): boolean {
	const result = toRecord(response.result);
	return (
		method === "session/prompt" &&
		response.id === null &&
		response.error === undefined &&
		result.stopReason === "cancelled"
	);
}

const CLOSED_SESSION_ID_RETENTION_LIMIT = 2048;
const CLOSED_SHELL_ID_RETENTION_LIMIT = 2048;

class BoundedSet<T> {
	readonly limit: number;
	#entries = new Map<T, undefined>();

	constructor(limit: number) {
		if (!Number.isInteger(limit) || limit <= 0) {
			throw new Error(`BoundedSet limit must be a positive integer: ${limit}`);
		}
		this.limit = limit;
	}

	add(value: T): void {
		if (this.#entries.has(value)) {
			this.#entries.delete(value);
		}
		this.#entries.set(value, undefined);
		if (this.#entries.size <= this.limit) {
			return;
		}
		const oldest = this.#entries.keys().next();
		if (!oldest.done) {
			this.#entries.delete(oldest.value);
		}
	}

	has(value: T): boolean {
		return this.#entries.has(value);
	}

	delete(value: T): boolean {
		return this.#entries.delete(value);
	}

	get size(): number {
		return this.#entries.size;
	}
}

function shouldDispatchToSessionEventHandlers(
	notification: JsonRpcNotification,
): boolean {
	return notification.method === "session/update";
}

function toSessionModes(value: unknown): SessionModeState | null {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return null;
	}
	return value as SessionModeState;
}

function toSessionConfigOptions(value: unknown): SessionConfigOption[] {
	return Array.isArray(value) ? (value as SessionConfigOption[]) : [];
}

function toAgentCapabilities(value: unknown): AgentCapabilities {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return {};
	}
	return value as AgentCapabilities;
}

function toAgentInfo(value: unknown): AgentInfo | null {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return null;
	}
	if (typeof (value as { name?: unknown }).name !== "string") {
		return null;
	}
	return value as AgentInfo;
}

function sessionEntryFromInit(
	sessionId: string,
	agentType: string,
	initData: SessionInitData,
): AgentSessionEntry {
	return {
		sessionId,
		agentType,
		processId: "",
		pid: null,
		closed: false,
		modes: initData.modes ?? null,
		configOptions: initData.configOptions ?? [],
		capabilities: initData.capabilities ?? {},
		agentInfo: initData.agentInfo ?? null,
		eventHandlers: new Set(),
		permissionHandlers: new Set(),
		configOverrides: new Map(),
		pendingPermissionReplies: new Map(),
	};
}

function isOverlayMountConfig(
	config: MountConfig,
): config is OverlayMountConfig {
	return "filesystem" in config;
}

function isNativeMountConfig(config: MountConfig): config is NativeMountConfig {
	return "plugin" in config;
}

interface HostDirMountPluginConfig {
	hostPath: string;
	readOnly?: boolean;
}

interface SandboxAgentMountPluginConfig {
	baseUrl: string;
	token?: string;
	headers?: Record<string, string>;
	basePath?: string;
	timeoutMs?: number;
	maxFullReadBytes?: number;
}

interface S3MountPluginCredentials {
	accessKeyId: string;
	secretAccessKey: string;
}

interface GoogleDriveMountPluginCredentials {
	clientEmail: string;
	privateKey: string;
}

interface S3MountPluginConfig {
	bucket: string;
	prefix?: string;
	region?: string;
	credentials?: S3MountPluginCredentials;
	endpoint?: string;
	chunkSize?: number;
	inlineThreshold?: number;
}

interface GoogleDriveMountPluginConfig {
	credentials: GoogleDriveMountPluginCredentials;
	folderId: string;
	keyPrefix?: string;
	chunkSize?: number;
	inlineThreshold?: number;
}

function asMountConfigJsonObject(
	value: MountConfigJsonValue | undefined,
): MountConfigJsonObject {
	if (value && typeof value === "object" && !Array.isArray(value)) {
		return value as MountConfigJsonObject;
	}
	return {};
}

function getHostDirMountPluginConfig(
	config: MountConfigJsonValue | undefined,
): HostDirMountPluginConfig | null {
	const object = asMountConfigJsonObject(config);
	if (typeof object.hostPath !== "string") {
		return null;
	}

	const hostPathConfig: HostDirMountPluginConfig = {
		hostPath: object.hostPath,
	};
	if (typeof object.readOnly === "boolean") {
		hostPathConfig.readOnly = object.readOnly;
	}
	return hostPathConfig;
}

function getSandboxAgentMountPluginConfig(
	config: MountConfigJsonValue | undefined,
): SandboxAgentMountPluginConfig | null {
	const object = asMountConfigJsonObject(config);
	if (typeof object.baseUrl !== "string") {
		return null;
	}

	const sandboxConfig: SandboxAgentMountPluginConfig = {
		baseUrl: object.baseUrl,
	};
	if (typeof object.token === "string") {
		sandboxConfig.token = object.token;
	}
	if (typeof object.basePath === "string") {
		sandboxConfig.basePath = object.basePath;
	}
	if (typeof object.timeoutMs === "number") {
		sandboxConfig.timeoutMs = object.timeoutMs;
	}
	if (typeof object.maxFullReadBytes === "number") {
		sandboxConfig.maxFullReadBytes = object.maxFullReadBytes;
	}
	if (
		object.headers &&
		typeof object.headers === "object" &&
		!Array.isArray(object.headers)
	) {
		const headers = Object.entries(object.headers)
			.filter(([, value]) => typeof value === "string")
			.map(([name, value]) => [name, value as string]);
		if (headers.length > 0) {
			sandboxConfig.headers = Object.fromEntries(headers);
		}
	}

	return sandboxConfig;
}

function getS3MountPluginConfig(
	config: MountConfigJsonValue | undefined,
): S3MountPluginConfig | null {
	const object = asMountConfigJsonObject(config);
	if (typeof object.bucket !== "string") {
		return null;
	}

	const s3Config: S3MountPluginConfig = {
		bucket: object.bucket,
	};
	if (typeof object.prefix === "string") {
		s3Config.prefix = object.prefix;
	}
	if (typeof object.region === "string") {
		s3Config.region = object.region;
	}
	if (typeof object.endpoint === "string") {
		s3Config.endpoint = object.endpoint;
	}
	if (typeof object.chunkSize === "number") {
		s3Config.chunkSize = object.chunkSize;
	}
	if (typeof object.inlineThreshold === "number") {
		s3Config.inlineThreshold = object.inlineThreshold;
	}
	if (
		object.credentials &&
		typeof object.credentials === "object" &&
		!Array.isArray(object.credentials) &&
		typeof object.credentials.accessKeyId === "string" &&
		typeof object.credentials.secretAccessKey === "string"
	) {
		s3Config.credentials = {
			accessKeyId: object.credentials.accessKeyId,
			secretAccessKey: object.credentials.secretAccessKey,
		};
	}

	return s3Config;
}

function getGoogleDriveMountPluginConfig(
	config: MountConfigJsonValue | undefined,
): GoogleDriveMountPluginConfig | null {
	const object = asMountConfigJsonObject(config);
	if (typeof object.folderId !== "string") {
		return null;
	}
	if (
		!object.credentials ||
		typeof object.credentials !== "object" ||
		Array.isArray(object.credentials) ||
		typeof object.credentials.clientEmail !== "string" ||
		typeof object.credentials.privateKey !== "string"
	) {
		return null;
	}

	const googleDriveConfig: GoogleDriveMountPluginConfig = {
		credentials: {
			clientEmail: object.credentials.clientEmail,
			privateKey: object.credentials.privateKey,
		},
		folderId: object.folderId,
	};
	if (typeof object.keyPrefix === "string") {
		googleDriveConfig.keyPrefix = object.keyPrefix;
	}
	if (typeof object.chunkSize === "number") {
		googleDriveConfig.chunkSize = object.chunkSize;
	}
	if (typeof object.inlineThreshold === "number") {
		googleDriveConfig.inlineThreshold = object.inlineThreshold;
	}

	return googleDriveConfig;
}

const KERNEL_POSIX_BOOTSTRAP_DIRS = [
	"/dev",
	"/proc",
	"/tmp",
	"/bin",
	"/lib",
	"/sbin",
	"/boot",
	"/etc",
	"/root",
	"/run",
	"/srv",
	"/sys",
	"/opt",
	"/mnt",
	"/media",
	"/home",
	"/home/agentos",
	"/workspace",
	"/usr",
	"/usr/bin",
	"/usr/games",
	"/usr/include",
	"/usr/lib",
	"/usr/libexec",
	"/usr/man",
	"/usr/local",
	"/usr/local/bin",
	"/usr/sbin",
	"/usr/share",
	"/usr/share/man",
	"/var",
	"/var/cache",
	"/var/empty",
	"/var/lib",
	"/var/lock",
	"/var/log",
	"/var/run",
	"/var/spool",
	"/var/tmp",
	"/etc/agentos",
] as const;

const NODE_RUNTIME_BOOTSTRAP_COMMANDS = ["node", "npm", "npx"] as const;
const KERNEL_COMMAND_STUB = "#!/bin/sh\n# kernel command stub\n";
const REPO_ROOT = fileURLToPath(new URL("../../..", import.meta.url));
const SIDECAR_BINARY = join(REPO_ROOT, "target/debug/agentos-sidecar");
const SIDECAR_BUILD_INPUTS = [
	join(REPO_ROOT, "Cargo.toml"),
	join(REPO_ROOT, "Cargo.lock"),
	join(REPO_ROOT, "crates/bridge"),
	join(REPO_ROOT, "crates/execution"),
	join(REPO_ROOT, "crates/kernel"),
	join(REPO_ROOT, "crates/sidecar"),
] as const;
let ensuredSidecarBinary: string | null = null;

interface PreparedCommandDirs {
	commandDirs: string[];
	dispose(): void;
}

function isWasmBinaryFile(path: string): boolean {
	try {
		const header = readFileSync(path);
		return (
			header.length >= 4 &&
			header[0] === 0x00 &&
			header[1] === 0x61 &&
			header[2] === 0x73 &&
			header[3] === 0x6d
		);
	} catch {
		return false;
	}
}

function collectBootstrapWasmCommands(commandDirs: string[]): string[] {
	const commands: string[] = [];
	const seen = new Set<string>();

	for (const dir of commandDirs) {
		let entries: string[];
		try {
			entries = readdirSync(dir).sort((a, b) => a.localeCompare(b));
		} catch {
			continue;
		}

		for (const entry of entries) {
			if (entry.startsWith(".")) {
				continue;
			}

			const fullPath = join(dir, entry);
			try {
				if (statSync(fullPath).isDirectory()) {
					continue;
				}
			} catch {
				continue;
			}

			if (!isWasmBinaryFile(fullPath) || seen.has(entry)) {
				continue;
			}

			seen.add(entry);
			commands.push(entry);
		}
	}

	return commands;
}

function resolveDeclaredCommandSource(
	commandDir: string,
	commandName: string,
	aliases: Record<string, string>,
): string | null {
	let current = commandName;
	const visited = new Set<string>();

	while (!visited.has(current)) {
		visited.add(current);

		const candidatePath = join(commandDir, current);
		if (isWasmBinaryFile(candidatePath)) {
			return candidatePath;
		}

		const next = aliases[current];
		if (!next) {
			return null;
		}

		current = next;
	}

	return null;
}

function prepareCommandDirs(
	commandPackages: CommandPackageMetadata[],
): PreparedCommandDirs {
	const commandDirs: string[] = [];
	const tempDirs: string[] = [];

	try {
		for (const commandPackage of commandPackages) {
			commandDirs.push(commandPackage.commandDir);

			const aliasEntries = Object.entries(commandPackage.aliases)
				.sort(([leftAlias], [rightAlias]) =>
					leftAlias.localeCompare(rightAlias),
				)
				.flatMap(([aliasName]) => {
					const aliasPath = join(commandPackage.commandDir, aliasName);
					if (isWasmBinaryFile(aliasPath)) {
						return [];
					}

					const sourcePath = resolveDeclaredCommandSource(
						commandPackage.commandDir,
						aliasName,
						commandPackage.aliases,
					);
					if (!sourcePath) {
						return [];
					}

					return [[aliasName, sourcePath] as const];
				});

			if (aliasEntries.length === 0) {
				continue;
			}

			const aliasDir = mkdtempSync(join(tmpdir(), "agentos-command-aliases-"));
			for (const [aliasName, sourcePath] of aliasEntries) {
				writeFileSync(join(aliasDir, aliasName), readFileSync(sourcePath));
			}

			tempDirs.push(aliasDir);
			commandDirs.push(aliasDir);
		}
	} catch (error) {
		for (const tempDir of tempDirs) {
			rmSync(tempDir, { recursive: true, force: true });
		}
		throw error;
	}

	return {
		commandDirs,
		dispose() {
			for (const tempDir of tempDirs) {
				rmSync(tempDir, { recursive: true, force: true });
			}
		},
	};
}

function collectConfiguredLowerPaths(
	config?: RootFilesystemConfig,
): Set<string> {
	const paths = new Set<string>();

	for (const lower of config?.lowers ?? []) {
		if (lower.kind !== "snapshot-export") {
			continue;
		}
		for (const entry of lower.source.filesystem.entries) {
			paths.add(entry.path);
		}
	}

	return paths;
}

function findBootstrapSeedEntry(
	config: RootFilesystemConfig | undefined,
	path: string,
): BaseFilesystemEntry | undefined {
	for (const lower of config?.lowers ?? []) {
		if (lower.kind !== "snapshot-export") {
			continue;
		}
		const entry = lower.source.filesystem.entries.find(
			(candidate) => candidate.path === path,
		);
		if (entry) {
			return entry;
		}
	}

	return getBaseFilesystemEntries().find((entry) => entry.path === path);
}

function createKernelBootstrapLower(
	config: RootFilesystemConfig | undefined,
	commandNames: string[],
	extraEntries: FilesystemEntry[] = [],
): RootSnapshotExport | null {
	const includesBundledBaseLayer = !(config?.disableDefaultBaseLayer ?? false);
	const existingPaths = collectConfiguredLowerPaths(config);
	if (includesBundledBaseLayer) {
		for (const entry of getBaseFilesystemEntries()) {
			existingPaths.add(entry.path);
		}
	}
	const entries: FilesystemEntry[] = [
		{
			path: "/",
			type: "directory",
			mode: "755",
			uid: 0,
			gid: 0,
		},
	];

	for (const dir of KERNEL_POSIX_BOOTSTRAP_DIRS) {
		if (existingPaths.has(dir)) {
			continue;
		}
		const seed = findBootstrapSeedEntry(config, dir);
		entries.push({
			path: dir,
			type: "directory",
			mode: seed?.type === "directory" ? seed.mode : "755",
			uid: seed?.uid ?? 0,
			gid: seed?.gid ?? 0,
		});
	}

	if (!includesBundledBaseLayer && !existingPaths.has("/usr/bin/env")) {
		entries.push({
			path: "/usr/bin/env",
			type: "file",
			mode: "644",
			uid: 0,
			gid: 0,
			content: "AA==",
			encoding: "base64",
		});
	}

	const uniqueCommands = [...new Set(commandNames)].sort((a, b) =>
		a.localeCompare(b),
	);
	for (const command of uniqueCommands) {
		const stubPath = `/bin/${command}`;
		if (existingPaths.has(stubPath)) {
			continue;
		}
		entries.push({
			path: stubPath,
			type: "file",
			mode: "755",
			uid: 0,
			gid: 0,
			content: KERNEL_COMMAND_STUB,
			encoding: "utf8",
		});
	}

	for (const entry of sortFilesystemEntries(extraEntries)) {
		if (existingPaths.has(entry.path)) {
			continue;
		}
		entries.push(entry);
	}

	return entries.length > 1 ? createSnapshotExport(entries) : null;
}

function buildLiveBootstrapDirectoryEntries(
	existingPaths: ReadonlySet<string>,
	config: RootFilesystemConfig | undefined,
): RootFilesystemEntry[] {
	const entries: RootFilesystemEntry[] = [];
	for (const dir of KERNEL_POSIX_BOOTSTRAP_DIRS) {
		if (existingPaths.has(dir)) {
			continue;
		}
		const seed = findBootstrapSeedEntry(config, dir);
		entries.push({
			path: dir,
			kind: "directory",
			mode: Number.parseInt(seed?.type === "directory" ? seed.mode : "755", 8),
			uid: seed?.uid ?? 0,
			gid: seed?.gid ?? 0,
			executable: true,
		});
	}
	return entries;
}

async function bootstrapLiveBootstrapDirectories(
	client: SidecarProcess,
	session: AuthenticatedSession,
	vm: CreatedVm,
	config: RootFilesystemConfig | undefined,
): Promise<void> {
	const existingPaths = new Set(
		(await client.snapshotRootFilesystem(session, vm)).map(
			(entry) => entry.path,
		),
	);
	const entries = buildLiveBootstrapDirectoryEntries(existingPaths, config);
	if (entries.length === 0) {
		return;
	}
	await client.bootstrapRootFilesystem(session, vm, entries);
}

function toSnapshotModeString(
	mode: number | undefined,
	kind: RootFilesystemEntry["kind"],
): string {
	const fallback =
		kind === "directory" ? 0o755 : kind === "symlink" ? 0o777 : 0o644;
	return `0${((mode ?? fallback) & 0o7777).toString(8)}`;
}

function convertSidecarRootSnapshotEntries(
	entries: RootFilesystemEntry[],
): FilesystemEntry[] {
	return entries.map((entry) => {
		const baseEntry: FilesystemEntry = {
			path: entry.path,
			type: entry.kind,
			mode: toSnapshotModeString(entry.mode, entry.kind),
			uid: entry.uid ?? 0,
			gid: entry.gid ?? 0,
		};

		if (entry.kind === "file") {
			return {
				...baseEntry,
				content: entry.content ?? "",
				encoding: entry.encoding ?? "utf8",
			};
		}

		if (entry.kind === "symlink") {
			if (entry.target === undefined) {
				throw new Error(
					`sidecar root snapshot for ${entry.path} is missing a symlink target`,
				);
			}
			return {
				...baseEntry,
				target: entry.target,
			};
		}

		return baseEntry;
	});
}

function ensureNativeSidecarBinary(): string {
	// A published install has no in-repo Cargo workspace to build from: resolve
	// the prebuilt platform binary (or the AGENTOS_SIDECAR_BIN override).
	if (
		process.env.AGENTOS_SIDECAR_BIN ||
		!existsSync(join(REPO_ROOT, "Cargo.toml"))
	) {
		return resolvePublishedSidecarBinary();
	}
	if (
		ensuredSidecarBinary &&
		existsSync(ensuredSidecarBinary) &&
		!sidecarBinaryNeedsBuild()
	) {
		return ensuredSidecarBinary;
	}

	if (sidecarBinaryNeedsBuild()) {
		const cargoBinary = findCargoBinary();
		if (cargoBinary) {
			execFileSync(cargoBinary, ["build", "-q", "-p", "agentos-sidecar"], {
				cwd: REPO_ROOT,
				stdio: "pipe",
			});
		} else if (!existsSync(SIDECAR_BINARY)) {
			execFileSync(
				resolveCargoBinary(),
				["build", "-q", "-p", "agentos-sidecar"],
				{
					cwd: REPO_ROOT,
					stdio: "pipe",
				},
			);
		}
	}

	ensuredSidecarBinary = SIDECAR_BINARY;
	return ensuredSidecarBinary;
}

function sidecarBinaryNeedsBuild(): boolean {
	if (!existsSync(SIDECAR_BINARY)) {
		return true;
	}

	const binaryMtimeMs = statSync(SIDECAR_BINARY).mtimeMs;
	return SIDECAR_BUILD_INPUTS.some(
		(path) => existsSync(path) && latestMtimeMs(path) > binaryMtimeMs,
	);
}

function latestMtimeMs(path: string): number {
	const stats = statSync(path);
	if (!stats.isDirectory()) {
		return stats.mtimeMs;
	}

	let latest = stats.mtimeMs;
	for (const entry of readdirSync(path)) {
		latest = Math.max(latest, latestMtimeMs(join(path, entry)));
	}
	return latest;
}

function collectGuestCommandPaths(commandDirs: string[]): Map<string, string> {
	const guestPaths = new Map<string, string>();

	for (const [index, commandDir] of commandDirs.entries()) {
		let entries: string[];
		try {
			entries = readdirSync(commandDir).sort((left, right) =>
				left.localeCompare(right),
			);
		} catch {
			continue;
		}

		for (const entry of entries) {
			if (entry.startsWith(".")) {
				continue;
			}
			if (!isWasmBinaryFile(join(commandDir, entry)) || guestPaths.has(entry)) {
				continue;
			}
			guestPaths.set(entry, `/__secure_exec/commands/${index}/${entry}`);
		}
	}

	return guestPaths;
}

async function resolveCompatLocalMounts(
	mounts?: MountConfig[],
): Promise<LocalCompatMount[]> {
	if (!mounts) {
		return [];
	}

	const resolved: LocalCompatMount[] = [];
	for (const mount of mounts) {
		if (isNativeMountConfig(mount)) {
			continue;
		}

		if (!isOverlayMountConfig(mount)) {
			resolved.push({
				path: posixPath.normalize(mount.path),
				fs: mount.driver,
				readOnly: mount.readOnly ?? false,
			});
			continue;
		}

		const mode = mount.filesystem.mode ?? "ephemeral";
		const fs =
			mode === "read-only"
				? mount.filesystem.store.createOverlayFilesystem({
						mode: "read-only",
						lowers: mount.filesystem.lowers,
					})
				: mount.filesystem.store.createOverlayFilesystem({
						upper: await mount.filesystem.store.createWritableLayer(),
						lowers: mount.filesystem.lowers,
					});

		resolved.push({
			path: posixPath.normalize(mount.path),
			fs,
			readOnly: mode === "read-only",
		});
	}

	return resolved;
}

function collectSidecarMountPlan(options: {
	mounts?: MountConfig[];
	softwareRoots: SoftwareRoot[];
	commandDirs: string[];
	shimDir: string | null;
}): {
	sidecarMounts: Array<ReturnType<typeof serializeMountConfigForSidecar>>;
	hostMounts: HostMountInfo[];
	hostPathMappings: HostMountInfo[];
} {
	const sidecarMounts: Array<
		ReturnType<typeof serializeMountConfigForSidecar>
	> = [];
	const hostMounts: HostMountInfo[] = [];
	const hostPathMappings: HostMountInfo[] = [];
	const seenMounts = new Set<string>();

	function pushMount(mount: NativeMountConfig): void {
		const serialized = serializeMountConfigForSidecar(mount);
		const key = `${serialized.guestPath}\0${serialized.plugin.id}\0${JSON.stringify(
			serialized.plugin.config,
		)}`;
		if (seenMounts.has(key)) {
			return;
		}
		seenMounts.add(key);
		sidecarMounts.push(serialized);

		if (mount.plugin.id === "host_dir") {
			const config = getHostDirMountPluginConfig(mount.plugin.config);
			if (config) {
				hostPathMappings.push({
					vmPath: posixPath.normalize(mount.path),
					hostPath: resolveHostPath(config.hostPath),
					readOnly: mount.readOnly ?? config.readOnly ?? true,
				});
			}
			if (config && options.mounts?.some((candidate) => candidate === mount)) {
				hostMounts.push({
					vmPath: posixPath.normalize(mount.path),
					hostPath: resolveHostPath(config.hostPath),
					readOnly: mount.readOnly ?? config.readOnly ?? true,
				});
			}
		}
	}

	for (const mount of options.mounts ?? []) {
		if (!isNativeMountConfig(mount)) {
			sidecarMounts.push({
				guestPath: mount.path,
				readOnly: isOverlayMountConfig(mount)
					? (mount.filesystem.mode ?? "ephemeral") === "read-only"
					: (mount.readOnly ?? false),
				plugin: {
					id: "js_bridge",
					config: {},
				},
			});
			continue;
		}
		pushMount(mount);
	}

	for (const root of options.softwareRoots) {
		pushMount({
			path: root.vmPath,
			plugin: createHostDirBackend({
				hostPath: root.hostPath,
				readOnly: true,
			}),
			readOnly: true,
		});
	}

	for (const [index, commandDir] of options.commandDirs.entries()) {
		pushMount({
			path: `/__secure_exec/commands/${index}`,
			plugin: createHostDirBackend({
				hostPath: commandDir,
				readOnly: true,
			}),
			readOnly: true,
		});
	}

	if (options.shimDir) {
		pushMount({
			path: "/usr/local/bin",
			plugin: createHostDirBackend({
				hostPath: options.shimDir,
				readOnly: true,
			}),
			readOnly: true,
		});
	}

	hostMounts.sort((left, right) => right.vmPath.length - left.vmPath.length);
	hostPathMappings.sort(
		(left, right) => right.vmPath.length - left.vmPath.length,
	);
	return { sidecarMounts, hostMounts, hostPathMappings };
}

function materializeToolShimDir(toolKits: ToolKit[]): string {
	const shimDir = mkdtempSync(join(tmpdir(), "agentos-host-tools-shims-"));
	writeFileSync(join(shimDir, "agentos"), KERNEL_COMMAND_STUB, { mode: 0o755 });

	for (const toolKit of toolKits) {
		writeFileSync(
			join(shimDir, `agentos-${toolKit.name}`),
			KERNEL_COMMAND_STUB,
			{ mode: 0o755 },
		);
	}

	return shimDir;
}

function collectToolkitBootstrapCommands(toolKits: ToolKit[]): string[] {
	if (toolKits.length === 0) {
		return [];
	}

	return ["agentos", ...toolKits.map((toolKit) => `agentos-${toolKit.name}`)];
}

function validationMessage(error: unknown): string {
	if (
		typeof error === "object" &&
		error !== null &&
		"issues" in error &&
		Array.isArray((error as { issues?: unknown[] }).issues)
	) {
		return (
			error as { issues: Array<{ message: string; path?: unknown[] }> }
		).issues
			.map((issue) => {
				const path =
					Array.isArray(issue.path) && issue.path.length > 0
						? ` at "${issue.path.join(".")}"`
						: "";
				return `${issue.message}${path}`;
			})
			.join("; ");
	}
	return error instanceof Error ? error.message : String(error);
}

function toolToSidecarDefinition(
	tool: HostTool,
): SidecarRegisteredHostCallbackDefinition {
	return {
		description: tool.description,
		inputSchema: zodToJsonSchema(tool.inputSchema),
		...(tool.timeout !== undefined ? { timeoutMs: tool.timeout } : {}),
		...(tool.examples && tool.examples.length > 0
			? {
					examples: tool.examples.map((example) => ({
						description: example.description,
						input: example.input,
					})),
				}
			: {}),
	};
}

function combineInstructions(
	additionalInstructions: string | undefined,
	toolReference: string,
): string | null {
	const parts = [additionalInstructions, toolReference]
		.map((part) => part?.trim())
		.filter((part): part is string => Boolean(part));
	if (parts.length === 0) {
		return null;
	}
	return parts.join("\n\n");
}

function buildHostToolReference(toolKits: ToolKit[]): string {
	if (toolKits.length === 0) {
		return "";
	}

	const lines = [
		"## Available Host Tools",
		"",
		"Run `agentos list-tools` to see all available tools.",
		"",
	];

	for (const toolKit of toolKits) {
		lines.push(`### ${toolKit.name}`);
		lines.push("");
		lines.push(toolKit.description);
		lines.push("");
		for (const [toolName, tool] of Object.entries(toolKit.tools)) {
			const sidecarTool = toolToSidecarDefinition(tool);
			const signature = buildToolFlagSignature(sidecarTool.inputSchema);
			const suffix = signature.length > 0 ? ` ${signature}` : "";
			lines.push(
				`- \`agentos-${toolKit.name} ${toolName}${suffix}\` — ${tool.description}`,
			);
		}
		lines.push("");

		const toolsWithExamples = Object.entries(toolKit.tools).filter(
			([, tool]) => tool.examples && tool.examples.length > 0,
		);
		if (toolsWithExamples.length > 0) {
			lines.push("**Examples:**");
			lines.push("");
			for (const [toolName, tool] of toolsWithExamples) {
				for (const example of tool.examples ?? []) {
					const args = inputToToolFlags(example.input);
					const suffix = args.length > 0 ? ` ${args}` : "";
					lines.push(
						`- ${example.description}: \`agentos-${toolKit.name} ${toolName}${suffix}\``,
					);
				}
			}
			lines.push("");
		}

		lines.push(`Run \`agentos-${toolKit.name} <tool> --help\` for details.`);
		lines.push("");
	}

	return lines.join("\n");
}

function buildToolFlagSignature(schema: unknown): string {
	return describeToolFlags(schema)
		.map((flag) => {
			if (flag.required) {
				return `${flag.name} <${flag.type}>`;
			}
			return `[${flag.name} <${flag.type}>]`;
		})
		.join(" ");
}

function describeToolFlags(
	schema: unknown,
): Array<{ name: string; type: string; required: boolean }> {
	const schemaObject = asRecord(schema);
	const properties = asRecord(schemaObject.properties);
	const required = Array.isArray(schemaObject.required)
		? new Set(
				schemaObject.required.filter(
					(item): item is string => typeof item === "string",
				),
			)
		: new Set<string>();

	return Object.entries(properties).map(([fieldName, fieldSchema]) => ({
		name: `--${camelToKebab(fieldName)}`,
		type: describeToolFlagType(fieldSchema),
		required: required.has(fieldName),
	}));
}

function describeToolFlagType(schema: unknown): string {
	const schemaObject = asRecord(schema);
	const type =
		typeof schemaObject.type === "string" ? schemaObject.type : undefined;
	if (type === "array") {
		const itemType = describeJsonSchemaScalarType(schemaObject.items);
		return `${itemType}[]`;
	}
	if (type === "string") {
		const enumValues = Array.isArray(schemaObject.enum)
			? schemaObject.enum.filter(
					(item): item is string => typeof item === "string",
				)
			: [];
		return enumValues.length > 0 ? enumValues.join("|") : "string";
	}
	return type ?? "string";
}

function describeJsonSchemaScalarType(schema: unknown): string {
	const schemaObject = asRecord(schema);
	return typeof schemaObject.type === "string" ? schemaObject.type : "string";
}

function inputToToolFlags(input: unknown): string {
	const inputObject = asRecord(input);
	return Object.entries(inputObject)
		.flatMap(([key, value]) => {
			const flag = `--${camelToKebab(key)}`;
			if (value === true) {
				return [flag];
			}
			if (value === false) {
				return [`--no-${camelToKebab(key)}`];
			}
			if (Array.isArray(value)) {
				return value.map((item) => `${flag} ${toolCliString(item)}`);
			}
			return [`${flag} ${toolCliString(value)}`];
		})
		.join(" ");
}

function toolCliString(value: unknown): string {
	return typeof value === "string" ? value : (JSON.stringify(value) ?? "null");
}

function camelToKebab(value: string): string {
	return value.replace(
		/[A-Z]/g,
		(ch, index) => `${index > 0 ? "-" : ""}${ch.toLowerCase()}`,
	);
}

function asRecord(value: unknown): Record<string, unknown> {
	if (typeof value === "object" && value !== null && !Array.isArray(value)) {
		return value as Record<string, unknown>;
	}
	return {};
}

async function handleHostCallback(
	request: SidecarRequestFrame,
	context: HostCallbackContext,
): Promise<SidecarResponsePayload> {
	const payload = request.payload;
	if (payload.type !== "host_callback") {
		return {
			type: "host_callback_result",
			invocation_id: "unknown",
			error: `unsupported sidecar request type: ${payload.type}`,
		};
	}

	const command = parseHostCommandCallbackInput(payload.input);
	if (command) {
		try {
			return {
				type: "host_callback_result",
				invocation_id: payload.invocation_id,
				result: await handleHostCommandCallback(command, context),
			};
		} catch (error) {
			return {
				type: "host_callback_result",
				invocation_id: payload.invocation_id,
				error: validationMessage(error),
			};
		}
	}

	const tool = context.toolMap.get(payload.callback_key);
	if (!tool) {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			error: `Unknown tool "${payload.callback_key}"`,
		};
	}

	const permissionMode = toolPermissionMode(
		context.permissions,
		payload.callback_key,
	);
	if (permissionMode !== "allow") {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			error: `EACCES: blocked by binding.invoke policy for ${payload.callback_key}`,
		};
	}

	const parsed = tool.inputSchema.safeParse(payload.input);
	if (!parsed.success) {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			error: validationMessage(parsed.error),
		};
	}

	try {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			result: await executeHostTool(tool, payload.callback_key, parsed.data),
		};
	} catch (error) {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			error: validationMessage(error),
		};
	}
}

function buildToolMap(toolKits: ToolKit[]): Map<string, HostTool> {
	const toolMap = new Map<string, HostTool>();
	for (const toolKit of toolKits) {
		for (const [toolName, tool] of Object.entries(toolKit.tools)) {
			toolMap.set(`${toolKit.name}:${toolName}`, tool);
		}
	}
	return toolMap;
}

interface HostCommandCallbackInput {
	type: "command";
	command: string;
	args: string[];
	cwd: string;
}

interface HostCallbackContext {
	toolKits: ToolKit[];
	toolMap: ReadonlyMap<string, HostTool>;
	permissions: Permissions;
	readFile(path: string): Promise<Uint8Array>;
}

interface JsBridgeContext {
	filesystem: VirtualFileSystem;
}

function bridgeErrorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function toBridgeArgs(value: unknown): Record<string, unknown> {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new Error("js_bridge args must be an object");
	}
	return value as Record<string, unknown>;
}

function bridgePath(mountId: string, value: unknown): string {
	if (!mountId.startsWith("/")) {
		throw new Error(`Unsupported js_bridge mount id: ${mountId}`);
	}
	if (typeof value !== "string") {
		throw new Error("js_bridge path argument must be a string");
	}
	return posixPath.normalize(posixPath.join(mountId, value));
}

function requireBridgeNumber(value: unknown, field: string): number {
	if (typeof value !== "number" || !Number.isFinite(value)) {
		throw new Error(`js_bridge args.${field} must be a number`);
	}
	return value;
}

function decodeBridgeBytes(value: unknown, field: string): Uint8Array {
	if (typeof value === "string") {
		return new Uint8Array(Buffer.from(value, "base64"));
	}
	if (
		Array.isArray(value) &&
		value.every(
			(entry) => Number.isInteger(entry) && entry >= 0 && entry <= 255,
		)
	) {
		return new Uint8Array(value);
	}
	throw new Error(`js_bridge args.${field} must be base64 bytes`);
}

async function handleJsBridgeCall(
	request: Extract<SidecarRequestFrame["payload"], { type: "js_bridge_call" }>,
	context: JsBridgeContext,
): Promise<SidecarResponsePayload> {
	try {
		const args = toBridgeArgs(request.args);
		const fs = context.filesystem;
		const path = () => bridgePath(request.mount_id, args.path);
		let result: unknown;

		switch (request.operation) {
			case "readFile":
				result = Buffer.from(await fs.readFile(path())).toString("base64");
				break;
			case "readDir":
				result = await fs.readDir(path());
				break;
			case "readDirWithTypes":
				result = await fs.readDirWithTypes(path());
				break;
			case "writeFile":
				await fs.writeFile(path(), decodeBridgeBytes(args.content, "content"));
				break;
			case "createDir":
				await fs.createDir(path());
				break;
			case "mkdir":
				await fs.mkdir(path(), { recursive: args.recursive !== false });
				break;
			case "exists":
				result = await fs.exists(path());
				break;
			case "stat":
				result = await fs.stat(path());
				break;
			case "removeFile":
				await fs.removeFile(path());
				break;
			case "removeDir":
				await fs.removeDir(path());
				break;
			case "rename":
				await fs.rename(
					bridgePath(request.mount_id, args.oldPath),
					bridgePath(request.mount_id, args.newPath),
				);
				break;
			case "realpath":
				result = await fs.realpath(path());
				break;
			case "symlink": {
				if (typeof args.target !== "string") {
					throw new Error("js_bridge args.target must be a string");
				}
				await fs.symlink(
					args.target,
					bridgePath(request.mount_id, args.linkPath),
				);
				break;
			}
			case "readlink":
				result = await fs.readlink(path());
				break;
			case "lstat":
				result = await fs.lstat(path());
				break;
			case "link":
				await fs.link(
					bridgePath(request.mount_id, args.oldPath),
					bridgePath(request.mount_id, args.newPath),
				);
				break;
			case "chmod":
				await fs.chmod(path(), requireBridgeNumber(args.mode, "mode"));
				break;
			case "chown":
				await fs.chown(
					path(),
					requireBridgeNumber(args.uid, "uid"),
					requireBridgeNumber(args.gid, "gid"),
				);
				break;
			case "utimes":
				await fs.utimes(
					path(),
					requireBridgeNumber(args.atimeMs, "atimeMs"),
					requireBridgeNumber(args.mtimeMs, "mtimeMs"),
				);
				break;
			case "truncate":
				await fs.truncate(path(), requireBridgeNumber(args.length, "length"));
				break;
			case "pread":
				result = Buffer.from(
					await fs.pread(
						path(),
						requireBridgeNumber(args.offset, "offset"),
						requireBridgeNumber(args.length, "length"),
					),
				).toString("base64");
				break;
			case "pwrite":
				await fs.pwrite(
					path(),
					requireBridgeNumber(args.offset, "offset"),
					decodeBridgeBytes(args.content, "content"),
				);
				break;
			default:
				throw new Error(
					`Unsupported js_bridge operation: ${request.operation}`,
				);
		}

		return {
			type: "js_bridge_result",
			call_id: request.call_id,
			...(result === undefined ? {} : { result }),
		};
	} catch (error) {
		return {
			type: "js_bridge_result",
			call_id: request.call_id,
			error: bridgeErrorMessage(error),
		};
	}
}

function parseHostCommandCallbackInput(
	input: unknown,
): HostCommandCallbackInput | null {
	const value = asRecord(input);
	if (
		value.type !== "command" ||
		typeof value.command !== "string" ||
		typeof value.cwd !== "string" ||
		!Array.isArray(value.args) ||
		!value.args.every((arg): arg is string => typeof arg === "string")
	) {
		return null;
	}
	return {
		type: "command",
		command: value.command,
		args: value.args,
		cwd: value.cwd,
	};
}

async function handleHostCommandCallback(
	command: HostCommandCallbackInput,
	context: HostCallbackContext,
): Promise<unknown> {
	const directToolKit = context.toolKits.find(
		(toolKit) => `agentos-${toolKit.name}` === command.command,
	);
	if (command.command === "agentos") {
		return handleAgentOsRegistryCommand(command, context);
	}
	if (directToolKit) {
		return handleAgentOsToolkitCommand(command, context, directToolKit);
	}
	throw new Error(`Unknown host callback command "${command.command}"`);
}

async function handleAgentOsRegistryCommand(
	command: HostCommandCallbackInput,
	context: HostCallbackContext,
): Promise<unknown> {
	const [subcommand, toolkitName, toolName, ...toolArgs] = command.args;
	if (!subcommand || isHelpFlag(subcommand)) {
		return {
			usage:
				"agentos <command>: list-tools [toolkit], <toolkit> --help, or <toolkit> <tool> ...",
		};
	}
	if (subcommand === "list-tools") {
		return toolkitName
			? describeToolkitPayload(context.toolKits, toolkitName)
			: listToolkitsPayload(context.toolKits);
	}
	const toolKit = context.toolKits.find((kit) => kit.name === subcommand);
	if (!toolKit) {
		throw new Error(
			`No toolkit "${subcommand}". Available: ${toolkitNames(context.toolKits)}`,
		);
	}
	if (!toolkitName || isHelpFlag(toolkitName)) {
		return describeToolkitPayload(context.toolKits, subcommand);
	}
	if (toolName && isHelpFlag(toolName)) {
		return describeToolPayload(toolKit, toolkitName);
	}
	return invokeHostTool({
		toolKit,
		toolName: toolkitName,
		args: [toolName, ...toolArgs].filter(
			(value): value is string => typeof value === "string",
		),
		cwd: command.cwd,
		context,
	});
}

async function handleAgentOsToolkitCommand(
	command: HostCommandCallbackInput,
	context: HostCallbackContext,
	toolKit: ToolKit,
): Promise<unknown> {
	const [toolName, helpOrFirstArg, ...rest] = command.args;
	if (!toolName || isHelpFlag(toolName)) {
		return describeToolkitPayload(context.toolKits, toolKit.name);
	}
	if (helpOrFirstArg && isHelpFlag(helpOrFirstArg)) {
		return describeToolPayload(toolKit, toolName);
	}
	return invokeHostTool({
		toolKit,
		toolName,
		args: [helpOrFirstArg, ...rest].filter(
			(value): value is string => typeof value === "string",
		),
		cwd: command.cwd,
		context,
	});
}

async function invokeHostTool({
	toolKit,
	toolName,
	args,
	cwd,
	context,
}: {
	toolKit: ToolKit;
	toolName: string;
	args: string[];
	cwd: string;
	context: HostCallbackContext;
}): Promise<unknown> {
	const tool = toolKit.tools[toolName];
	if (!tool) {
		throw new Error(
			`No tool "${toolName}" in toolkit "${toolKit.name}". Available: ${toolNames(toolKit)}`,
		);
	}
	const callbackKey = `${toolKit.name}:${toolName}`;
	const permissionMode = toolPermissionMode(context.permissions, callbackKey);
	if (permissionMode !== "allow") {
		throw new Error(
			`EACCES: blocked by binding.invoke policy for ${callbackKey}`,
		);
	}
	const input = await parseHostToolInput(tool, args, cwd, context.readFile);
	return executeHostTool(tool, callbackKey, input);
}

async function executeHostTool(
	tool: HostTool,
	callbackKey: string,
	input: unknown,
): Promise<unknown> {
	const parsed = tool.inputSchema.safeParse(input);
	if (!parsed.success) {
		throw new Error(validationMessage(parsed.error));
	}

	return Promise.race([
		Promise.resolve(tool.execute(parsed.data)),
		new Promise<never>((_, reject) => {
			if (tool.timeout === undefined) {
				return;
			}
			setTimeout(
				() =>
					reject(
						new Error(
							`Tool "${callbackKey}" timed out after ${tool.timeout}ms`,
						),
					),
				tool.timeout,
			);
		}),
	]);
}

async function parseHostToolInput(
	tool: HostTool,
	args: string[],
	cwd: string,
	readFile: (path: string) => Promise<Uint8Array>,
): Promise<unknown> {
	if (args[0] === "--json") {
		const value = args[1];
		if (value === undefined) {
			throw new Error("Flag --json requires a value");
		}
		return JSON.parse(value);
	}
	if (args[0] === "--json-file") {
		const value = args[1];
		if (value === undefined) {
			throw new Error("Flag --json-file requires a value");
		}
		const guestPath = value.startsWith("/")
			? posixPath.normalize(value)
			: posixPath.normalize(`${cwd}/${value}`);
		const text = new TextDecoder().decode(await readFile(guestPath));
		return JSON.parse(text);
	}
	return parseToolArgv(toolToSidecarDefinition(tool).inputSchema, args);
}

function parseToolArgv(
	schema: unknown,
	argv: string[],
): Record<string, unknown> {
	const schemaObject = asRecord(schema);
	const properties = asRecord(schemaObject.properties);
	const required = Array.isArray(schemaObject.required)
		? new Set(
				schemaObject.required.filter(
					(value): value is string => typeof value === "string",
				),
			)
		: new Set<string>();
	const flagToField = new Map<string, [string, unknown]>();
	for (const [fieldName, fieldSchema] of Object.entries(properties)) {
		flagToField.set(camelToKebab(fieldName), [fieldName, fieldSchema]);
	}

	const input: Record<string, unknown> = {};
	for (let index = 0; index < argv.length; ) {
		const arg = argv[index];
		if (!arg?.startsWith("--")) {
			throw new Error(`Unexpected positional argument: "${arg}"`);
		}
		const rawFlag = arg.slice(2);
		const negated = rawFlag.startsWith("no-");
		const flagName = negated ? rawFlag.slice(3) : rawFlag;
		const entry = flagToField.get(flagName);
		if (!entry) {
			throw new Error(`Unknown flag: --${rawFlag}`);
		}
		const [fieldName, fieldSchema] = entry;
		const fieldType = jsonSchemaType(fieldSchema);
		if (negated) {
			if (fieldType !== "boolean") {
				throw new Error(`Unknown flag: --${rawFlag}`);
			}
			input[fieldName] = false;
			index += 1;
			continue;
		}
		if (fieldType === "boolean") {
			input[fieldName] = true;
			index += 1;
			continue;
		}
		const value = argv[index + 1];
		if (value === undefined) {
			throw new Error(`Flag --${rawFlag} requires a value`);
		}
		if (fieldType === "number" || fieldType === "integer") {
			const number = Number(value);
			if (!Number.isFinite(number)) {
				throw new Error(`Flag --${rawFlag} expects a number, got "${value}"`);
			}
			input[fieldName] = number;
			index += 2;
			continue;
		}
		if (fieldType === "array") {
			const current = Array.isArray(input[fieldName])
				? (input[fieldName] as unknown[])
				: [];
			const itemSchema = asRecord(fieldSchema).items;
			const itemType = jsonSchemaType(itemSchema);
			current.push(
				itemType === "number" || itemType === "integer" ? Number(value) : value,
			);
			input[fieldName] = current;
			index += 2;
			continue;
		}
		input[fieldName] = value;
		index += 2;
	}

	for (const fieldName of required) {
		if (!(fieldName in input)) {
			throw new Error(`Missing required flag: --${camelToKebab(fieldName)}`);
		}
	}
	return input;
}

function listToolkitsPayload(toolKits: ToolKit[]): unknown {
	return {
		toolkits: toolKits.map((toolKit) => ({
			name: toolKit.name,
			description: toolKit.description,
			tools: Object.keys(toolKit.tools),
		})),
	};
}

function describeToolkitPayload(
	toolKits: ToolKit[],
	toolkitName: string,
): unknown {
	const toolKit = toolKits.find((kit) => kit.name === toolkitName);
	if (!toolKit) {
		throw new Error(
			`No toolkit "${toolkitName}". Available: ${toolkitNames(toolKits)}`,
		);
	}
	return {
		name: toolKit.name,
		description: toolKit.description,
		tools: Object.fromEntries(
			Object.entries(toolKit.tools).map(([toolName, tool]) => [
				toolName,
				{
					description: tool.description,
					flags: describeToolFlags(toolToSidecarDefinition(tool).inputSchema),
				},
			]),
		),
	};
}

function describeToolPayload(toolKit: ToolKit, toolName: string): unknown {
	const tool = toolKit.tools[toolName];
	if (!tool) {
		throw new Error(
			`No tool "${toolName}" in toolkit "${toolKit.name}". Available: ${toolNames(toolKit)}`,
		);
	}
	return {
		toolkit: toolKit.name,
		tool: toolName,
		description: tool.description,
		flags: describeToolFlags(toolToSidecarDefinition(tool).inputSchema),
		examples:
			tool.examples?.map((example) => ({
				description: example.description,
				input: example.input,
			})) ?? [],
	};
}

function toolPermissionMode(
	permissions: Permissions,
	callbackKey: string,
): "allow" | "deny" {
	const scope = permissions.binding;
	if (!scope) {
		return "deny";
	}
	if (typeof scope === "string") {
		return scope;
	}
	let mode: "allow" | "deny" = scope.default ?? "deny";
	for (const rule of scope.rules) {
		const operations = rule.operations ?? ["*"];
		const patterns = rule.patterns ?? ["**"];
		if (
			operations.some(
				(operation) => operation === "*" || operation === "invoke",
			) &&
			patterns.some((pattern) => permissionPatternMatches(pattern, callbackKey))
		) {
			mode = rule.mode;
		}
	}
	return mode;
}

function permissionPatternMatches(pattern: string, value: string): boolean {
	if (pattern === "*" || pattern === "**" || pattern === value) {
		return true;
	}
	const parts = pattern.split(/(\*\*|\*)/u);
	const source = parts
		.map((part) => {
			if (part === "**") return ".*";
			if (part === "*") return "[^:]*";
			return part.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
		})
		.join("");
	return new RegExp(`^${source}$`).test(value);
}

function toolkitNames(toolKits: ToolKit[]): string {
	return toolKits.map((toolKit) => toolKit.name).join(", ");
}

function toolNames(toolKit: ToolKit): string {
	return Object.keys(toolKit.tools).join(", ");
}

function isHelpFlag(value: string): boolean {
	return value === "--help" || value === "-h";
}

function jsonSchemaType(schema: unknown): string | undefined {
	const schemaObject = asRecord(schema);
	return typeof schemaObject.type === "string" ? schemaObject.type : undefined;
}

async function registerToolkitsOnSidecar(
	client: SidecarProcess,
	session: AuthenticatedSession,
	vm: CreatedVm,
	toolKits: ToolKit[],
): Promise<string> {
	if (toolKits.length === 0) {
		return "";
	}

	for (const toolKit of toolKits) {
		await client.registerHostCallbacks(session, vm, {
			name: toolKit.name,
			description: toolKit.description,
			commandAliases: [`agentos-${toolKit.name}`],
			registryCommandAliases: ["agentos"],
			callbacks: Object.fromEntries(
				Object.entries(toolKit.tools).map(([toolName, tool]) => [
					toolName,
					toolToSidecarDefinition(tool),
				]),
			),
		});
	}

	return buildHostToolReference(toolKits);
}

export class AgentOs {
	#kernel: Kernel;
	readonly sidecar: AgentOsSidecar;
	private _sessions = new Map<string, AgentSessionEntry>();
	private _closedSessionIds = new BoundedSet<string>(
		CLOSED_SESSION_ID_RETENTION_LIMIT,
	);
	private _sessionClosePromises = new Map<string, Promise<void>>();
	private _pendingSessionRequestResolvers = new Map<
		string,
		Set<{
			method: string;
			resolve: (response: JsonRpcResponse) => void;
		}>
	>();
	private _processes = new Map<
		number,
		{
			proc: ManagedProcess;
			command: string;
			args: string[];
			stdoutHandlers: Set<(data: Uint8Array) => void>;
			stderrHandlers: Set<(data: Uint8Array) => void>;
			exitHandlers: Set<(exitCode: number) => void>;
		}
	>();
	private _shells = new Map<string, ShellEntry>();
	private _closedShellIds = new BoundedSet<string>(
		CLOSED_SHELL_ID_RETENTION_LIMIT,
	);
	private _pendingShellExitPromises = new Set<Promise<void>>();
	private _shellCounter = 0;
	private _acpTerminals = new Map<string, AcpTerminalEntry>();
	private _acpTerminalCounter = 0;
	private _softwareRoots: SoftwareRoot[];
	private _softwareAgentConfigs: Map<string, AgentConfig>;
	private _cronManager!: CronManager;
	private _toolKits: ToolKit[] = [];
	private _toolReference = "";
	private _permissions: Permissions = allowAll;
	private _hostMounts: HostMountInfo[];
	private _env: Record<string, string>;
	private _rootFilesystem: VirtualFileSystem;
	private readonly _additionalInstructions: string | undefined;
	private _sidecarLease: AgentOsSidecarVmLease<AgentOsVmAdmin> | null = null;
	private readonly _sidecarClient: SidecarProcess;
	private readonly _sidecarSession: AuthenticatedSession;
	private readonly _sidecarVm: CreatedVm;
	private readonly _disposeSidecarEventListener: () => void;
	private readonly _agentStderrHandler?: AgentStderrHandler;
	private readonly _limitWarningHandler?: LimitWarningHandler;

	private constructor(
		kernel: Kernel,
		sidecar: AgentOsSidecar,
		softwareRoots: SoftwareRoot[],
		softwareAgentConfigs: Map<string, AgentConfig>,
		hostMounts: HostMountInfo[],
		env: Record<string, string>,
		rootFilesystem: VirtualFileSystem,
		sidecarClient: SidecarProcess,
		sidecarSession: AuthenticatedSession,
		sidecarVm: CreatedVm,
		additionalInstructions?: string,
		agentStderrHandler?: AgentStderrHandler,
		limitWarningHandler?: LimitWarningHandler,
	) {
		this.#kernel = kernel;
		this.sidecar = sidecar;
		this._softwareRoots = softwareRoots;
		this._softwareAgentConfigs = softwareAgentConfigs;
		this._hostMounts = hostMounts;
		this._env = env;
		this._rootFilesystem = rootFilesystem;
		this._sidecarClient = sidecarClient;
		this._sidecarSession = sidecarSession;
		this._sidecarVm = sidecarVm;
		this._additionalInstructions = additionalInstructions;
		this._agentStderrHandler = agentStderrHandler;
		this._limitWarningHandler = limitWarningHandler;
		this._disposeSidecarEventListener = this._sidecarClient.onEvent((event) => {
			this._handleSidecarEvent(event);
		});
		agentOsRuntimeAdmins.set(this, {
			kernel,
			rootView: rootFilesystem,
			env,
			sidecar,
		});
	}

	static async createSidecar(
		options: AgentOsCreateSidecarOptions = {},
	): Promise<AgentOsSidecar> {
		return createAgentOsSidecarInternal(options);
	}

	static async getSharedSidecar(
		options: AgentOsSharedSidecarOptions = {},
	): Promise<AgentOsSidecar> {
		return getSharedAgentOsSidecarInternal(options);
	}

	static async create(options?: AgentOsOptions): Promise<AgentOs> {
		options = parseAgentOsOptions(options);
		const software =
			options?.defaultSoftware === false
				? (options.software ?? [])
				: [commonSoftware, ...(options?.software ?? [])];
		const processed = processSoftware(software);
		// Agent-SDK snapshot bundle (loaded once per sidecar into the V8 startup
		// snapshot, reused across sessions) for any snapshot-enabled agent.
		const snapshotUserlandCode = resolveAgentSnapshotBundle(software);
		const localMounts = await resolveCompatLocalMounts(options?.mounts);
		const toolKits = options?.toolKits;
		if (toolKits && toolKits.length > 0) {
			validateToolkits(toolKits);
		}

		// Resolve the sidecar handle up front so every VM created here leases the
		// one shared native sidecar process owned by that handle.
		const sidecar = resolveAgentOsSidecar(options?.sidecar);

		const createVmAdmin = async (): Promise<AgentOsVmAdmin> => {
			const preparedCommandDirs = prepareCommandDirs(processed.commandPackages);
			const toolBootstrapCommands = collectToolkitBootstrapCommands(
				toolKits ?? [],
			);
			const bootstrapLower = createKernelBootstrapLower(
				options?.rootFilesystem,
				[
					...collectBootstrapWasmCommands(preparedCommandDirs.commandDirs),
					...NODE_RUNTIME_BOOTSTRAP_COMMANDS,
					...toolBootstrapCommands,
				],
			);
			let toolReference = "";
			let rootBridge: NativeSidecarKernelProxy | null = null;
			let kernel: Kernel | null = null;
			let client: SidecarProcess | null = null;
			let createdNativeVm: CreatedVm | null = null;
			let nativeSession: AuthenticatedSession | null = null;
			let toolShimDir: string | null = null;
			let cleanedUp = false;

			const cleanup = async (): Promise<void> => {
				if (cleanedUp) {
					return;
				}
				cleanedUp = true;
				if (toolShimDir) {
					rmSync(toolShimDir, { recursive: true, force: true });
					toolShimDir = null;
				}
				preparedCommandDirs.dispose();
			};

			try {
				const env: Record<string, string> = getBaseEnvironment();
				if (toolKits && toolKits.length > 0) {
					toolShimDir = materializeToolShimDir(toolKits);
				}
				const commandGuestPaths = collectGuestCommandPaths(
					preparedCommandDirs.commandDirs,
				);
				const requestedMounts = options?.moduleAccessCwd
					? [
							...(options.mounts ?? []),
							{
								path: "/root/node_modules",
								plugin: createHostDirBackend({
									hostPath: join(options.moduleAccessCwd, "node_modules"),
									readOnly: true,
								}),
								readOnly: true,
							},
						]
					: options?.mounts;
				const { sidecarMounts, hostMounts, hostPathMappings } =
					collectSidecarMountPlan({
						mounts: requestedMounts,
						softwareRoots: processed.softwareRoots,
						commandDirs: preparedCommandDirs.commandDirs,
						shimDir: toolShimDir,
					});
				// Reuse the sidecar handle's single shared native process; this VM
				// becomes another tenant of it rather than spawning its own process.
				const shared = await ensureSharedSidecarNativeProcess(sidecar);
				client = shared.client;
				const session = shared.session;
				nativeSession = session;
				const hostPermissions = options?.permissions ?? {
					...allowAll,
					binding: "allow",
				};
				const sidecarPermissions =
					serializePermissionsForSidecar(hostPermissions);
				const createVmConfig: CreateVmConfig = {
					env,
					rootFilesystem: serializeRootFilesystemForSidecar(
						options?.rootFilesystem,
						bootstrapLower,
					),
					permissions: sidecarPermissions,
					limits: options?.limits,
					loopbackExemptPorts: options?.loopbackExemptPorts ?? [],
					// 0.3: the Node builtin allow-list moved from configureVm to
					// VM creation. `undefined` => engine default allow-list;
					// `[]` => deny all; `[..]` => exactly those. Platform and
					// module resolution keep their engine defaults (full Node
					// emulation), matching the prior behavior where Agent OS only
					// constrained the builtin allow-list.
					...(options?.allowedNodeBuiltins !== undefined ||
					snapshotUserlandCode !== undefined
						? {
								jsRuntime: {
									platform: "node" as const,
									moduleResolution: "node" as const,
									...(options?.allowedNodeBuiltins !== undefined
										? { allowedBuiltins: options.allowedNodeBuiltins }
										: {}),
									...(snapshotUserlandCode !== undefined
										? { snapshotUserlandCode }
										: {}),
								},
							}
						: {}),
				};
				const nativeVm = await client.createVm(session, {
					runtime: "java_script",
					config: createVmConfig,
				});
				createdNativeVm = nativeVm;
				// Scope the readiness wait to THIS VM's ownership; on a shared process
				// other VMs are emitting their own lifecycle events concurrently.
				await client.waitForEvent(
					(event) =>
						event.payload.type === "vm_lifecycle" &&
						event.payload.state === "ready" &&
						event.ownership.scope === "vm" &&
						event.ownership.vm_id === nativeVm.vmId,
					10_000,
				);
				await client.configureVm(session, nativeVm, {
					mounts: sidecarMounts,
					permissions: sidecarPermissions,
					commandPermissions: processed.commandPermissions,
					loopbackExemptPorts: options?.loopbackExemptPorts,
				});
				if (toolKits && toolKits.length > 0) {
					toolReference = await registerToolkitsOnSidecar(
						client,
						session,
						nativeVm,
						toolKits,
					);
					commandGuestPaths.set("agentos", "/bin/agentos");
					for (const toolKit of toolKits) {
						commandGuestPaths.set(
							`agentos-${toolKit.name}`,
							`/bin/agentos-${toolKit.name}`,
						);
					}
				}

				rootBridge = new NativeSidecarKernelProxy({
					client,
					session,
					vm: nativeVm,
					env,
					cwd: "/workspace",
					localMounts,
					sidecarMounts,
					permissions: sidecarPermissions,
					commandPermissions: processed.commandPermissions,
					loopbackExemptPorts: options?.loopbackExemptPorts,
					commandGuestPaths,
					onDispose: cleanup,
					// The native process is owned by the AgentOsSidecar handle and
					// shared across VMs; disposing this VM must not kill the process.
					ownsClient: false,
				});
				await bootstrapLiveBootstrapDirectories(
					client,
					session,
					nativeVm,
					options?.rootFilesystem,
				);

				kernel = rootBridge as unknown as Kernel;
				const snapshotClient = client;

				return {
					env,
					hostMounts,
					kernel,
					rootView: rootBridge.createRootView(),
					sidecarMounts,
					sidecarPermissions,
					commandPermissions: processed.commandPermissions,
					loopbackExemptPorts: options?.loopbackExemptPorts,
					sidecarClient: client,
					sidecarSession: session,
					sidecarVm: nativeVm,
					permissions: hostPermissions,
					snapshotRootFilesystem: async () =>
						createSnapshotExport(
							convertSidecarRootSnapshotEntries(
								await snapshotClient.snapshotRootFilesystem(session, nativeVm),
							),
						),
					toolKits: toolKits ?? [],
					toolReference,
					async dispose() {
						if (kernel) {
							const currentKernel = kernel;
							kernel = null;
							await currentKernel.dispose();
						}
						if (rootBridge) {
							const currentRootBridge = rootBridge;
							rootBridge = null;
							await currentRootBridge.dispose();
							return;
						}
						await cleanup();
					},
				};
			} catch (error) {
				// The native process is shared and owned by the sidecar handle, so
				// never dispose the client here — only tear down this VM's resources.
				if (kernel) {
					await kernel.dispose().catch(() => {});
				}
				if (rootBridge) {
					await rootBridge.dispose().catch(() => {});
				} else {
					if (createdNativeVm && nativeSession && client) {
						await client
							.disposeVm(nativeSession, createdNativeVm)
							.catch(() => {});
					}
					await cleanup();
				}
				throw error;
			}
		};

		let sidecarLease: AgentOsSidecarVmLease<AgentOsVmAdmin> | null = null;

		try {
			sidecarLease = await leaseAgentOsSidecarVm(sidecar, {
				createVm: async () => createVmAdmin(),
			});
			const vmAdmin = sidecarLease.admin;

			const vm = new AgentOs(
				vmAdmin.kernel,
				sidecar,
				processed.softwareRoots,
				processed.agentConfigs,
				vmAdmin.hostMounts,
				vmAdmin.env,
				vmAdmin.rootView,
				vmAdmin.sidecarClient,
				vmAdmin.sidecarSession,
				vmAdmin.sidecarVm,
				options?.additionalInstructions,
				options?.onAgentStderr ?? defaultAgentStderrHandler,
				options?.onLimitWarning,
			);
			vm._sidecarLease = sidecarLease;
			vm._toolKits = vmAdmin.toolKits;
			vm._toolReference = vmAdmin.toolReference;
			vm._permissions = vmAdmin.permissions;
			vm._installSidecarRequestHandler();
			vm._cronManager = new CronManager(
				vm,
				options?.scheduleDriver ?? new TimerScheduleDriver(),
			);

			return vm;
		} catch (error) {
			await sidecarLease?.dispose().catch(() => {});
			throw error;
		}
	}

	async exec(
		command: string,
		options?: KernelExecOptions,
	): Promise<KernelExecResult> {
		return this.#kernel.exec(command, options);
	}

	async execArgv(
		command: string,
		args: readonly string[] = [],
		options?: KernelExecOptions,
	): Promise<KernelExecResult> {
		const kernel = this.#kernel as unknown as {
			execArgv(
				command: string,
				args?: readonly string[],
				options?: KernelExecOptions,
			): Promise<KernelExecResult>;
		};
		return kernel.execArgv(command, args, options);
	}

	private _trackProcess(
		proc: ManagedProcess,
		command: string,
		args: string[],
		stdoutHandlers: Set<(data: Uint8Array) => void>,
		stderrHandlers: Set<(data: Uint8Array) => void>,
		exitHandlers: Set<(exitCode: number) => void>,
	): { pid: number } {
		const entry = {
			proc,
			command,
			args,
			stdoutHandlers,
			stderrHandlers,
			exitHandlers,
		};
		this._processes.set(proc.pid, entry);

		// NOTE: do NOT delete from `_processes` on exit — the public API contract
		// (getProcess/listProcesses/stopProcess, see process-management.test.ts)
		// requires exited processes to stay queryable (running:false, exitCode set).
		// `_processes` is a process table for this VM's lifetime; it is freed wholesale
		// in dispose(). (H5: the leak was that dispose() never cleared it.)
		void proc.wait().then((code) => {
			for (const h of exitHandlers) h(code);
		});

		return { pid: proc.pid };
	}

	spawn(
		command: string,
		args: string[],
		options?: KernelSpawnOptions,
	): { pid: number } {
		const stdoutHandlers = new Set<(data: Uint8Array) => void>();
		const stderrHandlers = new Set<(data: Uint8Array) => void>();
		const exitHandlers = new Set<(exitCode: number) => void>();

		// Include caller-provided callbacks in the initial handler sets.
		if (options?.onStdout) stdoutHandlers.add(options.onStdout);
		if (options?.onStderr) stderrHandlers.add(options.onStderr);

		const proc = this.#kernel.spawn(command, args, {
			...options,
			onStdout: (data) => {
				for (const h of stdoutHandlers) h(data);
			},
			onStderr: (data) => {
				for (const h of stderrHandlers) h(data);
			},
		});

		return this._trackProcess(
			proc,
			command,
			args,
			stdoutHandlers,
			stderrHandlers,
			exitHandlers,
		);
	}

	/** Write data to a process's stdin. */
	writeProcessStdin(pid: number, data: string | Uint8Array): void {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		entry.proc.writeStdin(data);
	}

	/** Close a process's stdin stream. */
	closeProcessStdin(pid: number): void {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		entry.proc.closeStdin();
	}

	/** Subscribe to stdout data from a process. Returns an unsubscribe function. */
	onProcessStdout(
		pid: number,
		handler: (data: Uint8Array) => void,
	): () => void {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		entry.stdoutHandlers.add(handler);
		return () => {
			entry.stdoutHandlers.delete(handler);
		};
	}

	/** Subscribe to stderr data from a process. Returns an unsubscribe function. */
	onProcessStderr(
		pid: number,
		handler: (data: Uint8Array) => void,
	): () => void {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		entry.stderrHandlers.add(handler);
		return () => {
			entry.stderrHandlers.delete(handler);
		};
	}

	/** Subscribe to process exit. Returns an unsubscribe function. */
	onProcessExit(pid: number, handler: (exitCode: number) => void): () => void {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		// If already exited, call immediately.
		if (entry.proc.exitCode !== null) {
			handler(entry.proc.exitCode);
			return () => {};
		}
		entry.exitHandlers.add(handler);
		return () => {
			entry.exitHandlers.delete(handler);
		};
	}

	/** Wait for a process to exit. Returns the exit code. */
	waitProcess(pid: number): Promise<number> {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		return entry.proc.wait();
	}

	private _assertSafeAbsolutePath(path: string): void {
		if (!path.startsWith("/")) {
			throw new Error(`Path must be absolute: ${path}`);
		}
		if (posixPath.normalize(path) !== path) {
			throw new Error(`Path must be normalized: ${path}`);
		}
	}

	private _assertWritableAbsolutePath(path: string): void {
		this._assertSafeAbsolutePath(path);
		if (path === "/proc" || path.startsWith("/proc/")) {
			throw new Error(`Path is read-only: ${path}`);
		}
	}

	private _vfs(): VirtualFileSystem {
		return (this.#kernel as unknown as { vfs: VirtualFileSystem }).vfs;
	}

	private async _copyPath(from: string, to: string): Promise<void> {
		const stat = await this._vfs().lstat(from);
		if (stat.isSymbolicLink) {
			const target = await this._vfs().readlink(from);
			await this._vfs().symlink(target, to);
			return;
		}
		if (stat.isDirectory) {
			await this._mkdirp(posixPath.dirname(to));
			if (!(await this.#kernel.exists(to))) {
				await this.#kernel.mkdir(to);
			}
			await this._vfs().chmod(to, stat.mode);
			await this._vfs().chown(to, stat.uid, stat.gid);
			const entries = await this.#kernel.readdir(from);
			for (const entry of entries) {
				if (entry === "." || entry === "..") continue;
				const fromPath = from === "/" ? `/${entry}` : `${from}/${entry}`;
				const toPath = to === "/" ? `/${entry}` : `${to}/${entry}`;
				await this._copyPath(fromPath, toPath);
			}
			return;
		}
		const content = await this.#kernel.readFile(from);
		await this.writeFile(to, content);
		await this._vfs().chmod(to, stat.mode);
		await this._vfs().chown(to, stat.uid, stat.gid);
	}

	async readFile(path: string): Promise<Uint8Array> {
		this._assertSafeAbsolutePath(path);
		return this.#kernel.readFile(path);
	}

	async writeFile(path: string, content: string | Uint8Array): Promise<void> {
		this._assertWritableAbsolutePath(path);
		return this.#kernel.writeFile(path, content);
	}

	async writeFiles(entries: BatchWriteEntry[]): Promise<BatchWriteResult[]> {
		const results: BatchWriteResult[] = [];
		for (const entry of entries) {
			try {
				this._assertWritableAbsolutePath(entry.path);
				// Create parent directories as needed
				const parentDir = entry.path.substring(0, entry.path.lastIndexOf("/"));
				if (parentDir) {
					await this._mkdirp(parentDir);
				}
				await this.#kernel.writeFile(entry.path, entry.content);
				results.push({ path: entry.path, success: true });
			} catch (err: unknown) {
				results.push({
					path: entry.path,
					success: false,
					error: err instanceof Error ? err.message : String(err),
				});
			}
		}
		return results;
	}

	async readFiles(paths: string[]): Promise<BatchReadResult[]> {
		const results: BatchReadResult[] = [];
		for (const path of paths) {
			try {
				this._assertSafeAbsolutePath(path);
				const content = await this.#kernel.readFile(path);
				results.push({ path, content });
			} catch (err: unknown) {
				results.push({
					path,
					content: null,
					error: err instanceof Error ? err.message : String(err),
				});
			}
		}
		return results;
	}

	/** Recursively create directories (mkdir -p). */
	private async _mkdirp(path: string): Promise<void> {
		this._assertWritableAbsolutePath(path);
		// `kernel.mkdir` is already recursive (it defaults to recursive=true on both
		// the native sidecar and compat kernels) and creating an existing directory is
		// a no-op, so a single call is sufficient. Do NOT probe each ancestor with
		// `exists()` first: on the native sidecar every read-side op
		// (exists/stat/readFile) triggers a full shadow-tree walk, so a per-component
		// exists() loop makes `mkdir -p` cost O(components * tree).
		await this.#kernel.mkdir(path);
	}

	async mkdir(path: string, options?: { recursive?: boolean }): Promise<void> {
		if (options?.recursive) {
			return this._mkdirp(path);
		}
		this._assertSafeAbsolutePath(path);
		return this.#kernel.mkdir(path);
	}

	async readdir(path: string): Promise<string[]> {
		this._assertSafeAbsolutePath(path);
		return this.#kernel.readdir(path);
	}

	async readdirRecursive(
		path: string,
		options?: ReaddirRecursiveOptions,
	): Promise<DirEntry[]> {
		this._assertSafeAbsolutePath(path);
		const maxDepth = options?.maxDepth;
		const exclude = options?.exclude ? new Set(options.exclude) : undefined;
		const results: DirEntry[] = [];

		// BFS queue: [dirPath, currentDepth]
		const queue: [string, number][] = [[path, 0]];

		while (queue.length > 0) {
			const item = queue.shift();
			if (!item) break;
			const [dirPath, depth] = item;
			const entries = await this.#kernel.readdir(dirPath);

			for (const name of entries) {
				if (name === "." || name === "..") continue;
				if (exclude?.has(name)) continue;

				const fullPath = dirPath === "/" ? `/${name}` : `${dirPath}/${name}`;
				const s = await this.#kernel.stat(fullPath);

				if (s.isSymbolicLink) {
					results.push({
						path: fullPath,
						type: "symlink",
						size: s.size,
					});
				} else if (s.isDirectory) {
					results.push({
						path: fullPath,
						type: "directory",
						size: s.size,
					});
					if (maxDepth === undefined || depth < maxDepth) {
						queue.push([fullPath, depth + 1]);
					}
				} else {
					results.push({
						path: fullPath,
						type: "file",
						size: s.size,
					});
				}
			}
		}

		return results;
	}

	async stat(path: string): Promise<VirtualStat> {
		this._assertSafeAbsolutePath(path);
		return this.#kernel.stat(path);
	}

	async exists(path: string): Promise<boolean> {
		this._assertSafeAbsolutePath(path);
		return this.#kernel.exists(path);
	}

	async snapshotRootFilesystem(): Promise<RootSnapshotExport> {
		const nativeSnapshot = this._sidecarLease?.admin.snapshotRootFilesystem;
		if (nativeSnapshot) {
			return nativeSnapshot();
		}

		return createSnapshotExport(
			await snapshotVirtualFilesystem(this._rootFilesystem),
		);
	}

	mountFs(
		path: string,
		driver: VirtualFileSystem,
		options?: { readOnly?: boolean },
	): void {
		this._assertSafeAbsolutePath(path);
		this.#kernel.mountFs(path, driver, { readOnly: options?.readOnly });
	}

	unmountFs(path: string): void {
		this._assertSafeAbsolutePath(path);
		this.#kernel.unmountFs(path);
	}

	async move(from: string, to: string): Promise<void> {
		this._assertSafeAbsolutePath(from);
		this._assertSafeAbsolutePath(to);
		const sourceStat = await this._vfs().lstat(from);
		if (!sourceStat.isDirectory || sourceStat.isSymbolicLink) {
			return this.#kernel.rename(from, to);
		}
		await this._copyPath(from, to);
		await this.delete(from, { recursive: true });
	}

	async delete(path: string, options?: { recursive?: boolean }): Promise<void> {
		this._assertSafeAbsolutePath(path);
		const s = await this._vfs().lstat(path);
		if (s.isDirectory) {
			if (options?.recursive) {
				const entries = await this.#kernel.readdir(path);
				for (const entry of entries) {
					if (entry === "." || entry === "..") continue;
					await this.delete(`${path}/${entry}`, { recursive: true });
				}
			}
			return this.#kernel.removeDir(path);
		}
		return this.#kernel.removeFile(path);
	}

	async fetch(port: number, request: Request): Promise<Response> {
		const url = new URL(request.url);
		const responsePayload = JSON.parse(
			await this._sidecarClient.vmFetch(this._sidecarSession, this._sidecarVm, {
				port,
				method: request.method,
				path: `${url.pathname}${url.search}`,
				headersJson: JSON.stringify(
					Object.fromEntries(request.headers.entries()),
				),
				...(request.method !== "GET" && request.method !== "HEAD"
					? { body: await request.text() }
					: {}),
			}),
		) as {
			status: number;
			statusText?: string;
			headers?: Array<[string, string]>;
			body?: string;
		};
		const headers = new Headers();
		for (const [key, value] of responsePayload.headers ?? []) {
			headers.append(key, value);
		}
		return new Response(Buffer.from(responsePayload.body ?? "", "base64"), {
			status: responsePayload.status,
			statusText: responsePayload.statusText,
			headers,
		});
	}

	openShell(options?: OpenShellOptions): { shellId: string } {
		const shellId = `shell-${++this._shellCounter}`;
		this._closedShellIds.delete(shellId);
		const dataHandlers = new Set<(data: Uint8Array) => void>();

		const handle = this.#kernel.openShell(options);
		handle.onData = (data) => {
			for (const h of dataHandlers) h(data);
		};

		const entry: ShellEntry = {
			handle,
			dataHandlers,
			exitPromise: Promise.resolve(),
		};
		const exitPromise = handle.wait().then(
			() => undefined,
			() => undefined,
		);
		entry.exitPromise = exitPromise.finally(() => {
			this._pendingShellExitPromises.delete(entry.exitPromise);
			if (this._shells.get(shellId) === entry) {
				this._shells.delete(shellId);
				this._closedShellIds.add(shellId);
			}
		});
		this._pendingShellExitPromises.add(entry.exitPromise);
		this._shells.set(shellId, entry);
		return { shellId };
	}

	async connectTerminal(options?: ConnectTerminalOptions): Promise<number> {
		return this.#kernel.connectTerminal(options);
	}

	/** Write data to a shell's PTY input. */
	writeShell(shellId: string, data: string | Uint8Array): void {
		const entry = this._shells.get(shellId);
		if (!entry) throw new Error(`Shell not found: ${shellId}`);
		entry.handle.write(data);
	}

	/** Subscribe to data output from a shell. Returns an unsubscribe function. */
	onShellData(
		shellId: string,
		handler: (data: Uint8Array) => void,
	): () => void {
		const entry = this._shells.get(shellId);
		if (!entry) throw new Error(`Shell not found: ${shellId}`);
		entry.dataHandlers.add(handler);
		return () => {
			entry.dataHandlers.delete(handler);
		};
	}

	/** Notify a shell of terminal resize. */
	resizeShell(shellId: string, cols: number, rows: number): void {
		const entry = this._shells.get(shellId);
		if (!entry) throw new Error(`Shell not found: ${shellId}`);
		entry.handle.resize(cols, rows);
	}

	/** Kill a shell process and remove it from tracking. */
	closeShell(shellId: string): void {
		const entry = this._shells.get(shellId);
		if (!entry) {
			if (this._closedShellIds.has(shellId)) {
				return;
			}
			throw new Error(`Shell not found: ${shellId}`);
		}
		entry.handle.kill();
		this._shells.delete(shellId);
		this._closedShellIds.add(shellId);
	}

	private _resolveVmPathToHostPath(vmPath: string): string | null {
		const normalizedVmPath = posixPath.normalize(vmPath);
		for (const mount of this._hostMounts) {
			if (
				normalizedVmPath === mount.vmPath ||
				normalizedVmPath.startsWith(`${mount.vmPath}/`)
			) {
				const relativePath = posixPath.relative(mount.vmPath, normalizedVmPath);
				if (!relativePath) {
					return mount.hostPath;
				}
				return join(mount.hostPath, ...relativePath.split("/").filter(Boolean));
			}
		}
		return null;
	}

	/** Returns info about all processes spawned via spawn(). */
	listProcesses(): SpawnedProcessInfo[] {
		return [...this._processes.values()].map(({ proc, command, args }) => ({
			pid: proc.pid,
			command,
			args,
			running: proc.exitCode === null,
			exitCode: proc.exitCode,
		}));
	}

	/** Returns all kernel processes across all active runtimes (WASM and Node). */
	allProcesses(): KernelProcessInfo[] {
		if (this.#kernel instanceof NativeSidecarKernelProxy) {
			return this.#kernel.snapshotProcesses();
		}
		return [...this.#kernel.processes.values()];
	}

	/** Returns processes organized as a tree using ppid relationships. */
	processTree(): ProcessTreeNode[] {
		const all = this.allProcesses();
		const nodeMap = new Map<number, ProcessTreeNode>();

		// Index: create a tree node for each process
		for (const proc of all) {
			nodeMap.set(proc.pid, { ...proc, children: [] });
		}

		// Wire: attach each node to its parent
		const roots: ProcessTreeNode[] = [];
		for (const node of nodeMap.values()) {
			const parent = nodeMap.get(node.ppid);
			if (parent) {
				parent.children.push(node);
			} else {
				roots.push(node);
			}
		}

		return roots;
	}

	/** Returns info about a specific process by PID. Throws if not found. */
	getProcess(pid: number): SpawnedProcessInfo {
		const entry = this._processes.get(pid);
		if (!entry) {
			throw new Error(`Process not found: ${pid}`);
		}
		return {
			pid: entry.proc.pid,
			command: entry.command,
			args: entry.args,
			running: entry.proc.exitCode === null,
			exitCode: entry.proc.exitCode,
		};
	}

	/** Send SIGTERM to gracefully stop a process. No-op if already exited. */
	stopProcess(pid: number): void {
		const entry = this._processes.get(pid);
		if (!entry) {
			throw new Error(`Process not found: ${pid}`);
		}
		if (entry.proc.exitCode !== null) return;
		entry.proc.kill();
	}

	/** Send SIGKILL to force-kill a process. No-op if already exited. */
	killProcess(pid: number): void {
		const entry = this._processes.get(pid);
		if (!entry) {
			throw new Error(`Process not found: ${pid}`);
		}
		if (entry.proc.exitCode !== null) return;
		entry.proc.kill(9);
	}

	/** Returns all active sessions with their IDs and agent types. */
	listSessions(): SessionInfo[] {
		return [...this._sessions.values()].map((s) => ({
			sessionId: s.sessionId,
			agentType: s.agentType,
		}));
	}

	/** Internal helper: retrieve a session or throw. */
	private _requireSession(sessionId: string): AgentSessionEntry {
		const session = this._sessions.get(sessionId);
		if (!session) {
			throw new Error(`Session not found: ${sessionId}`);
		}
		return session;
	}

	/** Returns all registered agents with their installation status. */
	listAgents(): AgentRegistryEntry[] {
		// Collect all agent IDs from both package configs and hardcoded configs.
		const allIds = new Set<string>([
			...this._softwareAgentConfigs.keys(),
			...Object.keys(AGENT_CONFIGS),
		]);

		return [...allIds]
			.map((id) => {
				const config = this._resolveAgentConfig(id);
				if (!config) return null;

				let installed = false;
				try {
					// Check software roots first, then the host dir behind the
					// `/root/node_modules` mount.
					const vmPrefix = `/root/node_modules/${config.acpAdapter}`;
					let hostPkgJsonPath: string | null = null;
					for (const root of this._softwareRoots) {
						if (root.vmPath === vmPrefix) {
							hostPkgJsonPath = join(root.hostPath, "package.json");
							break;
						}
					}
					if (!hostPkgJsonPath) {
						const nodeModulesRoot = this._nodeModulesHostRoot();
						if (nodeModulesRoot) {
							hostPkgJsonPath = join(
								resolvePackageDir(nodeModulesRoot, config.acpAdapter),
								"package.json",
							);
						}
					}
					if (!hostPkgJsonPath) {
						throw new Error("no package source");
					}
					readFileSync(hostPkgJsonPath);
					installed = true;
				} catch {
					// Package not installed
				}
				return {
					id,
					acpAdapter: config.acpAdapter,
					agentPackage: config.agentPackage,
					installed,
				};
			})
			.filter((entry): entry is AgentRegistryEntry => entry !== null);
	}

	private _syncSessionState(
		session: AgentSessionEntry,
		state: Pick<
			SidecarSessionState,
			| "processId"
			| "pid"
			| "closed"
			| "modes"
			| "configOptions"
			| "agentCapabilities"
			| "agentInfo"
		>,
	): void {
		session.processId = state.processId;
		session.pid = state.pid ?? null;
		session.closed = state.closed;
		session.modes = toSessionModes(state.modes);
		session.configOptions = toSessionConfigOptions(state.configOptions);
		this._applySyntheticConfigOverrides(session);
		session.capabilities = toAgentCapabilities(state.agentCapabilities);
		session.agentInfo = toAgentInfo(state.agentInfo);
	}

	private _applySessionUpdate(
		session: AgentSessionEntry,
		notification: JsonRpcNotification,
	): void {
		if (notification.method !== "session/update") {
			return;
		}

		const params = toRecord(notification.params);
		const update = toRecord(params.update ?? params);
		const sessionUpdate = update.sessionUpdate;

		if (
			sessionUpdate === "current_mode_update" &&
			typeof update.currentModeId === "string" &&
			session.modes
		) {
			session.modes = {
				...session.modes,
				currentModeId: update.currentModeId,
			};
		}

		if (
			(sessionUpdate === "config_option_update" ||
				sessionUpdate === "config_options_update") &&
			Array.isArray(update.configOptions)
		) {
			session.configOptions = update.configOptions as SessionConfigOption[];
		}
	}

	private _recordSessionNotification(
		session: AgentSessionEntry,
		notification: JsonRpcNotification,
	): void {
		this._applySessionUpdate(session, notification);

		if (shouldDispatchToSessionEventHandlers(notification)) {
			this._dispatchSessionEvent(session, notification);
		}

		if (
			notification.method === LEGACY_PERMISSION_METHOD ||
			notification.method === ACP_PERMISSION_METHOD
		) {
			const params = toRecord(notification.params);
			const permissionId = params.permissionId;
			if (
				typeof permissionId === "string" ||
				typeof permissionId === "number"
			) {
				const request: PermissionRequest = {
					permissionId: String(permissionId),
					description:
						typeof params.description === "string"
							? params.description
							: undefined,
					params,
				};
				for (const handler of session.permissionHandlers) {
					handler(request);
				}
			}
		}
	}

	private _dispatchSessionEvent(
		session: AgentSessionEntry,
		notification: JsonRpcNotification,
	): void {
		if (session.eventHandlers.size === 0) {
			return;
		}
		for (const subscriber of [...session.eventHandlers]) {
			try {
				subscriber.handler(notification);
			} catch {
				// Ignore subscriber callback failures and keep event delivery moving.
			}
		}
	}

	private _subscribeSessionEvents(
		session: AgentSessionEntry,
		handler: SessionEventHandler,
	): () => void {
		const subscriber: SessionEventSubscriber = {
			handler,
		};
		session.eventHandlers.add(subscriber);
		return () => {
			session.eventHandlers.delete(subscriber);
		};
	}

	private _recordAgentStderr(event: {
		sessionId: string;
		agentType: string;
		processId: string;
		chunk: ArrayBuffer;
	}): void {
		const session =
			(event.sessionId ? this._sessions.get(event.sessionId) : undefined) ??
			[...this._sessions.values()].find(
				(candidate) => candidate.processId === event.processId,
			);
		const sessionId = event.sessionId || session?.sessionId;
		if (!sessionId) {
			return;
		}
		const handler = this._agentStderrHandler;
		if (!handler) {
			return;
		}
		try {
			handler({
				sessionId,
				agentType: event.agentType || session?.agentType || "",
				processId: event.processId,
				pid: session?.pid ?? null,
				chunk: new Uint8Array(event.chunk),
			});
		} catch {
			// Ignore subscriber callback failures and keep event delivery moving.
		}
	}

	private _applySyntheticConfigOverrides(session: AgentSessionEntry): void {
		if (session.configOverrides.size === 0) {
			return;
		}

		session.configOptions = session.configOptions.map((option) => {
			const override =
				session.configOverrides.get(option.id) ??
				(typeof option.category === "string"
					? session.configOverrides.get(option.category)
					: undefined);
			return override === undefined
				? option
				: { ...option, currentValue: override };
		});
	}

	private _handleSidecarEvent(
		event: Parameters<SidecarProcess["onEvent"]>[0] extends (
			event: infer T,
		) => void
			? T
			: never,
	): void {
		if (event.payload.type === "ext") {
			this._handleAcpExtEvent(event.payload.envelope);
			return;
		}
		if (event.payload.type !== "structured") {
			return;
		}
		if (event.payload.name === "limit_warning") {
			this._handleLimitWarning(event.payload.detail);
			return;
		}
		if (event.payload.name !== "acp.session_event") {
			return;
		}

		const sessionId = event.payload.detail.session_id;
		const session = sessionId ? this._sessions.get(sessionId) : undefined;
		if (!session) {
			return;
		}

		const notificationText = event.payload.detail.notification;
		if (typeof notificationText !== "string") {
			return;
		}

		try {
			this._recordSessionNotification(
				session,
				toJsonRpcNotification(JSON.parse(notificationText)),
			);
		} catch {
			// Ignore malformed event payloads from the sidecar.
		}
	}

	private _handleLimitWarning(detail: Record<string, string>): void {
		if (!this._limitWarningHandler) {
			return;
		}
		const toNumber = (value: string | undefined): number => {
			const parsed = Number(value);
			return Number.isFinite(parsed) ? parsed : 0;
		};
		try {
			this._limitWarningHandler({
				limit: detail.limit ?? "",
				category: detail.category ?? "",
				observed: toNumber(detail.observed),
				capacity: toNumber(detail.capacity),
				fillPercent: toNumber(detail.fillPercent),
			});
		} catch {
			// A throwing handler must never break the sidecar event loop.
		}
	}

	private _handleAcpExtEvent(envelope: {
		namespace: string;
		payload: Uint8Array;
	}): void {
		if (envelope.namespace !== ACP_EXTENSION_NAMESPACE) {
			return;
		}
		try {
			const event = decodeAcpEvent(envelope.payload);
			switch (event.tag) {
				case "AcpSessionEvent": {
					const session = this._sessions.get(event.val.sessionId);
					if (!session) {
						return;
					}
					this._recordSessionNotification(
						session,
						toJsonRpcNotification(JSON.parse(event.val.notification)),
					);
					return;
				}
				case "AcpAgentStderrEvent": {
					this._recordAgentStderr(event.val);
					return;
				}
			}
		} catch {
			// Ignore malformed event payloads from the sidecar.
		}
	}

	private _unsupportedConfigResponse(
		agentType: string,
		category: string,
	): JsonRpcResponse {
		const message =
			agentType === "opencode" && category === "model"
				? "OpenCode reports available models, but model switching must be configured before createSession() because ACP session/set_config_option is not implemented."
				: `The ${category} config option is read-only for ${agentType} sessions.`;
		return {
			jsonrpc: "2.0",
			id: null,
			error: {
				code: -32601,
				message,
			},
		};
	}

	private async _sendAcpRequest(request: AcpRequest): Promise<AcpResponse> {
		const envelope = await this._sidecarClient.extensionRequest(
			this._sidecarSession,
			this._sidecarVm,
			{
				namespace: ACP_EXTENSION_NAMESPACE,
				payload: encodeAcpRequest(request),
			},
		);
		if (envelope.namespace !== ACP_EXTENSION_NAMESPACE) {
			throw new Error(`unexpected ACP Ext namespace: ${envelope.namespace}`);
		}
		const response = decodeAcpResponse(envelope.payload);
		if (response.tag === "AcpErrorResponse") {
			const error = new Error(response.val.message) as Error & {
				code?: string;
			};
			error.code = response.val.code;
			throw error;
		}
		return response;
	}

	private async _sendSessionRequest(
		sessionId: string,
		method: string,
		params?: Record<string, unknown>,
	): Promise<JsonRpcResponse> {
		const session = this._requireSession(sessionId);
		const response = await new Promise<JsonRpcResponse>((resolve, reject) => {
			const resolvers =
				this._pendingSessionRequestResolvers.get(sessionId) ?? new Set();
			const resolver = {
				method,
				resolve: (response: JsonRpcResponse) => {
					resolve(response);
				},
			};
			resolvers.add(resolver);
			this._pendingSessionRequestResolvers.set(sessionId, resolvers);

			void this._sendAcpRequest({
				tag: "AcpSessionRequest",
				val: {
					sessionId,
					method,
					params: params === undefined ? null : JSON.stringify(params),
				},
			})
				.then((response) => {
					if (response.tag !== "AcpSessionRpcResponse") {
						throw new Error(
							`unexpected response to AcpSessionRequest: ${response.tag}`,
						);
					}
					return toJsonRpcResponse(JSON.parse(response.val.response));
				})
				.then(resolve, reject)
				.finally(() => {
					const nextResolvers =
						this._pendingSessionRequestResolvers.get(sessionId);
					if (!nextResolvers) {
						return;
					}
					nextResolvers.delete(resolver);
					if (nextResolvers.size === 0) {
						this._pendingSessionRequestResolvers.delete(sessionId);
					}
				});
		});
		const liveSession = this._sessions.get(sessionId);
		if (liveSession && !isLocalCancelledPromptResponse(method, response)) {
			await this._hydrateSessionState(liveSession).catch(() => {});
		}
		if (!response.error) {
			if (
				method === "session/set_mode" &&
				typeof params?.modeId === "string" &&
				session.modes
			) {
				session.modes = {
					...session.modes,
					currentModeId: params.modeId,
				};
			}
			if (
				method === "session/set_config_option" &&
				typeof params?.configId === "string" &&
				typeof params?.value === "string"
			) {
				const nextValue = params.value;
				const updatedOption = session.configOptions.find(
					(option) => option.id === params.configId,
				);
				session.configOverrides.set(params.configId, nextValue);
				if (typeof updatedOption?.category === "string") {
					session.configOverrides.set(updatedOption.category, nextValue);
				}
				session.configOptions = session.configOptions.map((option) =>
					option.id === params.configId
						? { ...option, currentValue: nextValue }
						: option,
				);
			}
		}
		return response;
	}

	private async _setSessionConfigByCategory(
		sessionId: string,
		category: string,
		value: string,
	): Promise<JsonRpcResponse> {
		const session = this._requireSession(sessionId);
		const option = session.configOptions.find(
			(entry) => entry.category === category,
		);
		if (option?.readOnly) {
			return this._unsupportedConfigResponse(session.agentType, category);
		}
		const response = await this._sendSessionRequest(
			sessionId,
			"session/set_config_option",
			{
				configId: option?.id ?? category,
				value,
			},
		);
		return response;
	}

	private _removeSession(sessionId: string): void {
		this._sessions.delete(sessionId);
	}

	private _abortPendingSessionRequests(sessionId: string): void {
		const resolvers = this._pendingSessionRequestResolvers.get(sessionId);
		if (!resolvers) {
			return;
		}
		this._pendingSessionRequestResolvers.delete(sessionId);
		const response: JsonRpcResponse = {
			jsonrpc: "2.0",
			id: null,
			error: {
				code: -32_000,
				message: `Session closed: ${sessionId}`,
			},
		};
		for (const resolver of resolvers) {
			resolver.resolve(response);
		}
	}

	private _cancelPendingPromptRequests(sessionId: string): boolean {
		const resolvers = this._pendingSessionRequestResolvers.get(sessionId);
		if (!resolvers) {
			return false;
		}

		const response: JsonRpcResponse = {
			jsonrpc: "2.0",
			id: null,
			result: {
				stopReason: "cancelled",
			},
		};

		let cancelledPrompt = false;
		for (const resolver of [...resolvers]) {
			if (resolver.method !== "session/prompt") {
				continue;
			}
			resolvers.delete(resolver);
			resolver.resolve(response);
			cancelledPrompt = true;
		}

		if (resolvers.size === 0) {
			this._pendingSessionRequestResolvers.delete(sessionId);
		}

		return cancelledPrompt;
	}

	private _rejectPendingPermissionReplies(sessionId: string): void {
		const session = this._sessions.get(sessionId);
		if (!session) {
			return;
		}
		this._rejectPendingPermissionRepliesFromSession(session);
	}

	private _rejectPendingPermissionRepliesFromSession(
		session: AgentSessionEntry,
	): void {
		for (const [
			permissionId,
			pendingReply,
		] of session.pendingPermissionReplies) {
			clearTimeout(pendingReply.timer);
			pendingReply.reject(
				new Error(`Session closed before permission reply: ${permissionId}`),
			);
		}
		session.pendingPermissionReplies.clear();
	}

	private async _closeSessionInternal(sessionId: string): Promise<void> {
		const closing = this._sessionClosePromises.get(sessionId);
		if (closing) {
			return closing;
		}
		if (this._closedSessionIds.has(sessionId)) {
			return;
		}

		this._abortPendingSessionRequests(sessionId);
		this._rejectPendingPermissionReplies(sessionId);

		this._requireSession(sessionId);
		this._removeSession(sessionId);
		this._closedSessionIds.add(sessionId);

		const closePromise = this._sendAcpRequest({
			tag: "AcpCloseSessionRequest",
			val: { sessionId },
		})
			.then((response) => {
				if (response.tag !== "AcpSessionClosedResponse") {
					throw new Error(
						`unexpected response to AcpCloseSessionRequest: ${response.tag}`,
					);
				}
			})
			.finally(() => {
				this._sessionClosePromises.delete(sessionId);
			});
		this._sessionClosePromises.set(sessionId, closePromise);
		await closePromise;
	}

	private async _hydrateSessionState(
		session: AgentSessionEntry,
	): Promise<void> {
		const response = await this._sendAcpRequest({
			tag: "AcpGetSessionStateRequest",
			val: { sessionId: session.sessionId },
		});
		if (response.tag !== "AcpSessionStateResponse") {
			throw new Error(
				`unexpected response to AcpGetSessionStateRequest: ${response.tag}`,
			);
		}
		const state = sidecarSessionStateFromAcp(response.val);
		this._syncSessionState(session, state);
	}

	async createSession(
		agentType: AgentType | string,
		options?: CreateSessionOptions,
	): Promise<{ sessionId: string }> {
		const config = this._resolveAgentConfig(agentType);
		if (!config) {
			throw new Error(`Unknown agent type: ${agentType}`);
		}

		// System-prompt assembly and injection (launch args / OPENCODE_CONTEXTPATHS) are owned by
		// the sidecar at AcpCreateSessionRequest. The host only forwards additionalInstructions /
		// skipOsInstructions plus the agent's static launch args and env.
		const launchArgs = [...(config.launchArgs ?? [])];
		let launchEnv = { ...config.defaultEnv, ...options?.env };
		const sessionCwd = options?.cwd ?? "/workspace";
		const adapterEntrypoint = this._resolveAdapterBin(config.acpAdapter);
		if (
			(agentType === "pi" || agentType === "pi-cli") &&
			!launchEnv.PI_ACP_PI_COMMAND
		) {
			launchEnv = {
				...launchEnv,
				PI_ACP_PI_COMMAND: this._resolvePackageBin(config.agentPackage, "pi"),
			};
		}

		const response = await this._sendAcpRequest({
			tag: "AcpCreateSessionRequest",
			val: {
				agentType: String(agentType),
				runtime: AcpRuntimeKind.JavaScript,
				adapterEntrypoint,
				args: launchArgs,
				env: new Map(Object.entries(launchEnv)),
				cwd: sessionCwd,
				mcpServers: JSON.stringify(options?.mcpServers ?? []),
				protocolVersion: ACP_PROTOCOL_VERSION,
				clientCapabilities: JSON.stringify(defaultAcpClientCapabilities()),
				additionalInstructions: combineInstructions(
					[this._additionalInstructions, options?.additionalInstructions]
						.map((part) => part?.trim())
						.filter((part): part is string => Boolean(part))
						.join("\n\n") || undefined,
					this._toolReference,
				),
				skipOsInstructions: options?.skipOsInstructions ?? false,
			},
		});
		if (response.tag !== "AcpSessionCreatedResponse") {
			throw new Error(`unexpected create_session response: ${response.tag}`);
		}
		const created = sidecarSessionCreatedFromAcp(response.val);

		// The sessionId is chosen by the (untrusted/buggy) ACP adapter or sidecar. If it collides
		// with a live session already in `_sessions`, blindly overwriting the entry would orphan the
		// first session's event/permission handlers and pending permission replies. Fail closed:
		// reject the colliding create and leave the original session intact. (A previously-closed,
		// evicted id may still be re-used; only a live, non-closed entry is a collision.)
		const existing = this._sessions.get(created.sessionId);
		if (existing !== undefined && !existing.closed) {
			throw new Error(`session id collision: ${created.sessionId}`);
		}

		const initData: SessionInitData = {
			modes: toSessionModes(created.modes) ?? undefined,
			configOptions: toSessionConfigOptions(created.configOptions),
			capabilities: toAgentCapabilities(created.agentCapabilities),
			agentInfo: toAgentInfo(created.agentInfo) ?? undefined,
		};
		const session = sessionEntryFromInit(
			created.sessionId,
			String(agentType),
			initData,
		);
		this._closedSessionIds.delete(created.sessionId);
		this._sessions.set(created.sessionId, session);

		try {
			await this._hydrateSessionState(session);
		} catch (error) {
			this._removeSession(created.sessionId);
			throw error;
		}

		return { sessionId: created.sessionId };
	}

	/**
	 * Resume a session that exists in durable storage but is not live in this VM
	 * (e.g. after a Rivet actor slept and woke with a fresh VM). Thin forwarder:
	 * resolves the agent config + adapter entrypoint exactly as {@link createSession}
	 * does, then forwards a single `AcpResumeSessionRequest` to the sidecar, which
	 * owns the resume state machine (native `session/load` when the agent supports
	 * it, else `session/new` + a transcript-continuation preamble). The returned
	 * `sessionId` is the live id in this VM (equal to the requested id for native
	 * loads, freshly assigned for the fallback); the caller remaps `external -> live`.
	 * The new live session is registered + hydrated locally so subsequent prompts
	 * route to it.
	 *
	 * Resume depends on a durable root; on a non-durable (default in-memory) root
	 * there is no surviving store and the fallback tier always runs.
	 */
	async resumeSession(
		sessionId: string,
		agentType: AgentType | string,
		options?: ResumeSessionOptions,
	): Promise<ResumeSessionResult> {
		const config = this._resolveAgentConfig(agentType);
		if (!config) {
			throw new Error(`Unknown agent type: ${agentType}`);
		}

		const adapterEntrypoint = this._resolveAdapterBin(config.acpAdapter);
		let launchEnv = { ...config.defaultEnv, ...options?.env };
		const sessionCwd = options?.cwd ?? "/workspace";
		if (
			(agentType === "pi" || agentType === "pi-cli") &&
			!launchEnv.PI_ACP_PI_COMMAND
		) {
			launchEnv = {
				...launchEnv,
				PI_ACP_PI_COMMAND: this._resolvePackageBin(config.agentPackage, "pi"),
			};
		}
		// The resume wire request has no dedicated `adapterEntrypoint` field; carry
		// the resolved entrypoint through env under the sidecar's reserved key. The
		// sidecar reads it and strips it before launching the adapter.
		launchEnv = {
			...launchEnv,
			[RESUME_ADAPTER_ENTRYPOINT_ENV]: adapterEntrypoint,
		};

		const response = await this._sendAcpRequest({
			tag: "AcpResumeSessionRequest",
			val: {
				sessionId,
				agentType: String(agentType),
				transcriptPath: options?.transcriptPath ?? null,
				cwd: sessionCwd,
				env: new Map(Object.entries(launchEnv)),
			},
		});
		if (response.tag !== "AcpSessionResumedResponse") {
			throw new Error(`unexpected resume_session response: ${response.tag}`);
		}
		const { sessionId: liveSessionId, mode } = response.val;

		// Register + hydrate the live session so subsequent prompts route to it.
		const existing = this._sessions.get(liveSessionId);
		if (existing !== undefined && !existing.closed) {
			throw new Error(`session id collision: ${liveSessionId}`);
		}

		const session = sessionEntryFromInit(liveSessionId, String(agentType), {});
		this._closedSessionIds.delete(liveSessionId);
		this._sessions.set(liveSessionId, session);
		try {
			await this._hydrateSessionState(session);
		} catch (error) {
			this._removeSession(liveSessionId);
			throw error;
		}

		return { sessionId: liveSessionId, mode };
	}

	/**
	 * Resolve the VM bin entry point of an ACP adapter package.
	 * Reads from the host filesystem since kernel.readFile() resolves through
	 * mounts; adapter package.json is read directly off the host dir backing
	 * the relevant mount.
	 */
	private _resolveAdapterBin(adapterPackage: string): string {
		return this._resolvePackageBin(adapterPackage);
	}

	/**
	 * Parent of the host directory backing the `/root/node_modules` mount, if
	 * the caller supplied one (e.g. via `nodeModulesMount(...)`). Used as the
	 * `resolvePackageDir` start dir for adapter/agent package resolution when no
	 * software root matches. `nodeModulesMount` mounts `<dir>/node_modules`, so
	 * the start dir is `<dir>` (one level above the node_modules tree).
	 */
	private _nodeModulesHostRoot(): string | null {
		for (const mount of this._hostMounts) {
			if (mount.vmPath === "/root/node_modules") {
				return dirname(mount.hostPath);
			}
		}
		return null;
	}

	private _resolvePackageBin(packageName: string, binName?: string): string {
		const vmPrefix = `/root/node_modules/${packageName}`;
		let hostPkgJsonPath: string | null = null;
		for (const root of this._softwareRoots) {
			if (root.vmPath === vmPrefix) {
				hostPkgJsonPath = join(root.hostPath, "package.json");
				break;
			}
		}
		// Fall back to the host dir behind the `/root/node_modules` mount.
		if (!hostPkgJsonPath) {
			const nodeModulesRoot = this._nodeModulesHostRoot();
			if (!nodeModulesRoot) {
				throw new Error(
					`Cannot resolve package "${packageName}": no software root provides it and ` +
						`no /root/node_modules mount was supplied. Pass ` +
						`mounts: [nodeModulesMount("<host>/node_modules")] to AgentOs.create().`,
				);
			}
			hostPkgJsonPath = join(
				resolvePackageDir(nodeModulesRoot, packageName),
				"package.json",
			);
		}
		const pkg = JSON.parse(readFileSync(hostPkgJsonPath, "utf-8"));

		let binEntry: string | undefined;
		if (typeof pkg.bin === "string") {
			binEntry = pkg.bin;
		} else if (typeof pkg.bin === "object" && pkg.bin !== null) {
			binEntry =
				(binName ? (pkg.bin as Record<string, string>)[binName] : undefined) ??
				(pkg.bin as Record<string, string>)[packageName] ??
				Object.values(pkg.bin)[0];
		}

		if (!binEntry) {
			throw new Error(`No bin entry found in ${packageName}/package.json`);
		}

		return `${vmPrefix}/${binEntry}`;
	}

	private _installSidecarRequestHandler(): void {
		const context: HostCallbackContext = {
			toolKits: this._toolKits,
			toolMap: buildToolMap(this._toolKits),
			permissions: this._permissions,
			readFile: (path) => this.readFile(path),
		};
		this._sidecarClient.setSidecarRequestHandler((request) => {
			switch (request.payload.type) {
				case "host_callback":
					return handleHostCallback(request, context);
				case "js_bridge_call":
					return handleJsBridgeCall(request.payload, {
						filesystem: this.#kernel.vfs,
					});
				case "ext":
					return this._handleAcpExtSidecarRequest(request.payload.envelope);
			}
		});
	}

	private async _handleAcpExtSidecarRequest(envelope: {
		namespace: string;
		payload: Uint8Array;
	}): Promise<SidecarResponsePayload> {
		if (envelope.namespace !== ACP_EXTENSION_NAMESPACE) {
			return {
				type: "ext_result",
				envelope: {
					namespace: envelope.namespace,
					payload: Buffer.from("unknown extension namespace", "utf8"),
				},
			};
		}
		const callback = decodeAcpCallback(envelope.payload);
		switch (callback.tag) {
			case "AcpPermissionCallback": {
				const reply = await this._handleAcpPermissionCallback(
					callback.val.sessionId,
					callback.val.permissionId,
					{
						...toRecord(JSON.parse(callback.val.params)),
						_acpMethod: ACP_PERMISSION_METHOD,
					},
				);
				return {
					type: "ext_result",
					envelope: {
						namespace: ACP_EXTENSION_NAMESPACE,
						payload: encodeAcpCallbackResponse({
							tag: "AcpPermissionCallbackResponse",
							val: {
								permissionId: callback.val.permissionId,
								reply,
							},
						}),
					},
				};
			}
			case "AcpHostRequestCallback": {
				const response = await this._dispatchAcpSidecarRequest(
					toJsonRpcRequest(JSON.parse(callback.val.request)),
				);
				return {
					type: "ext_result",
					envelope: {
						namespace: ACP_EXTENSION_NAMESPACE,
						payload: encodeAcpCallbackResponse({
							tag: "AcpHostRequestCallbackResponse",
							val: {
								response: JSON.stringify(response),
							},
						}),
					},
				};
			}
		}
	}

	private async _dispatchAcpSidecarRequest(
		request: JsonRpcRequest,
	): Promise<JsonRpcResponse> {
		try {
			const result = await this._handleSupportedAcpSidecarRequest(request);
			return {
				jsonrpc: "2.0",
				id: request.id,
				result,
			};
		} catch (error) {
			if (error instanceof AcpDispatchError) {
				return {
					jsonrpc: "2.0",
					id: request.id,
					error: {
						code: error.code,
						message: error.message,
						...(error.data ? { data: error.data } : {}),
					},
				};
			}
			return {
				jsonrpc: "2.0",
				id: request.id,
				error: {
					code: -32603,
					message: error instanceof Error ? error.message : String(error),
				},
			};
		}
	}

	private async _handleSupportedAcpSidecarRequest(
		request: JsonRpcRequest,
	): Promise<unknown> {
		const params = this._acpParams(request);
		switch (request.method) {
			case ACP_PERMISSION_METHOD:
				return this._handleAcpPermissionRequest(request, params);
			case "fs/read":
			case "fs/read_text_file":
				return this._handleAcpReadFile(params);
			case "fs/write":
			case "fs/write_text_file":
				return this._handleAcpWriteFile(params);
			case "fs/readDir":
			case "fs/read_dir":
				return this._handleAcpReadDir(params);
			case "terminal/create":
				return this._handleAcpCreateTerminal(params);
			case "terminal/write":
				return this._handleAcpWriteTerminal(params);
			case "terminal/output":
			case "terminal/read":
				return this._handleAcpReadTerminal(params);
			case "terminal/wait_for_exit":
			case "terminal/waitForExit":
				return this._handleAcpWaitForTerminalExit(params);
			case "terminal/kill":
				return this._handleAcpKillTerminal(params);
			case "terminal/release":
			case "terminal/close":
				return this._handleAcpReleaseTerminal(params);
			case "terminal/resize":
				return this._handleAcpResizeTerminal(params);
			default:
				throw new AcpDispatchError(
					-32601,
					`Method not found: ${request.method}`,
					{
						method: request.method,
					},
				);
		}
	}

	private _normalizeAcpPermissionOptionId(
		options: Array<Record<string, unknown>> | undefined,
		reply: PermissionReply,
	): string | null {
		const optionTargets =
			reply === "always"
				? {
						optionIds: new Set(["always", "allow_always"]),
						kinds: new Set(["allow_always"]),
					}
				: reply === "once"
					? {
							optionIds: new Set(["once", "allow_once"]),
							kinds: new Set(["allow_once"]),
						}
					: {
							optionIds: new Set(["reject", "reject_once"]),
							kinds: new Set(["reject_once"]),
						};

		const matched = options?.find((option) => {
			const optionId =
				typeof option.optionId === "string" ? option.optionId : undefined;
			const kind = typeof option.kind === "string" ? option.kind : undefined;
			return (
				(optionId !== undefined && optionTargets.optionIds.has(optionId)) ||
				(kind !== undefined && optionTargets.kinds.has(kind))
			);
		});
		if (matched && typeof matched.optionId === "string") {
			return matched.optionId;
		}
		if (reply === "always") {
			return "allow_always";
		}
		if (reply === "once") {
			return "allow_once";
		}
		return "reject_once";
	}

	private _buildAcpPermissionResult(
		reply: PermissionReply,
		params: Record<string, unknown>,
	): Record<string, unknown> {
		const options = Array.isArray(params.options)
			? params.options.filter(
					(option): option is Record<string, unknown> =>
						typeof option === "object" && option !== null,
				)
			: undefined;
		const optionId = this._normalizeAcpPermissionOptionId(options, reply);
		return {
			outcome: optionId
				? {
						outcome: "selected",
						optionId,
					}
				: {
						outcome: "cancelled",
					},
		};
	}

	private async _handleAcpPermissionRequest(
		request: JsonRpcRequest,
		params: Record<string, unknown>,
	): Promise<unknown> {
		const sessionId =
			typeof params.sessionId === "string" ? params.sessionId : undefined;
		if (!sessionId) {
			throw new AcpDispatchError(
				-32602,
				`${ACP_PERMISSION_METHOD} requires a sessionId`,
			);
		}

		const session = this._sessions.get(sessionId);
		if (!session) {
			throw new AcpDispatchError(-32602, `Session not found: ${sessionId}`);
		}

		const permissionId = String(request.id);
		const permissionParams: Record<string, unknown> = {
			...params,
			permissionId,
			_acpMethod: request.method,
		};
		if (session.permissionHandlers.size === 0) {
			return this._buildAcpPermissionResult("reject", permissionParams);
		}

		const reply = await new Promise<PermissionReply>((resolve, reject) => {
			const timer = setTimeout(() => {
				session.pendingPermissionReplies.delete(permissionId);
				reject(
					new Error(`Timed out waiting for permission reply: ${permissionId}`),
				);
			}, 120_000);
			session.pendingPermissionReplies.set(permissionId, {
				resolve,
				reject,
				timer,
			});

			const permissionRequest: PermissionRequest = {
				permissionId,
				description:
					typeof permissionParams["description"] === "string"
						? permissionParams["description"]
						: undefined,
				params: permissionParams,
			};
			for (const handler of session.permissionHandlers) {
				handler(permissionRequest);
			}
		});

		return this._buildAcpPermissionResult(reply, permissionParams);
	}

	private _acpParams(request: JsonRpcRequest): Record<string, unknown> {
		if (!request.params) {
			return {};
		}
		if (
			typeof request.params !== "object" ||
			request.params === null ||
			Array.isArray(request.params)
		) {
			throw new AcpDispatchError(
				-32602,
				`${request.method} requires object params`,
			);
		}
		return request.params as Record<string, unknown>;
	}

	private _requireAcpStringParam(
		params: Record<string, unknown>,
		name: string,
		method: string,
	): string {
		const value = params[name];
		if (typeof value !== "string") {
			throw new AcpDispatchError(-32602, `${method} requires a string ${name}`);
		}
		return value;
	}

	private _optionalAcpStringParam(
		params: Record<string, unknown>,
		name: string,
		method: string,
	): string | undefined {
		const value = params[name];
		if (value === undefined || value === null) {
			return undefined;
		}
		if (typeof value !== "string") {
			throw new AcpDispatchError(
				-32602,
				`${method} requires ${name} to be a string when provided`,
			);
		}
		return value;
	}

	private _optionalAcpNumberParam(
		params: Record<string, unknown>,
		name: string,
		method: string,
	): number | undefined {
		const value = params[name];
		if (value === undefined || value === null) {
			return undefined;
		}
		if (typeof value !== "number" || !Number.isFinite(value)) {
			throw new AcpDispatchError(
				-32602,
				`${method} requires ${name} to be a number when provided`,
			);
		}
		return value;
	}

	private _optionalAcpStringArrayParam(
		params: Record<string, unknown>,
		name: string,
		method: string,
	): string[] | undefined {
		const value = params[name];
		if (value === undefined || value === null) {
			return undefined;
		}
		if (
			!Array.isArray(value) ||
			value.some((entry) => typeof entry !== "string")
		) {
			throw new AcpDispatchError(
				-32602,
				`${method} requires ${name} to be an array of strings when provided`,
			);
		}
		return [...value];
	}

	private _optionalAcpEnvParam(
		params: Record<string, unknown>,
		name: string,
		method: string,
	): Record<string, string> | undefined {
		const value = params[name];
		if (value === undefined || value === null) {
			return undefined;
		}
		if (Array.isArray(value)) {
			const env: Record<string, string> = {};
			for (const entry of value) {
				if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
					throw new AcpDispatchError(
						-32602,
						`${method} requires ${name} entries to be { name, value } objects`,
					);
				}
				const record = entry as Record<string, unknown>;
				if (
					typeof record.name !== "string" ||
					typeof record.value !== "string"
				) {
					throw new AcpDispatchError(
						-32602,
						`${method} requires ${name} entries to be { name, value } objects`,
					);
				}
				env[record.name] = record.value;
			}
			return env;
		}
		if (typeof value !== "object") {
			throw new AcpDispatchError(
				-32602,
				`${method} requires ${name} to be an object or name/value array`,
			);
		}
		const env: Record<string, string> = {};
		for (const [key, entryValue] of Object.entries(
			value as Record<string, unknown>,
		)) {
			if (typeof entryValue !== "string") {
				throw new AcpDispatchError(
					-32602,
					`${method} requires ${name} values to be strings`,
				);
			}
			env[key] = entryValue;
		}
		return env;
	}

	private _requireAcpTerminal(
		params: Record<string, unknown>,
		method: string,
	): AcpTerminalEntry {
		const terminalId = this._requireAcpStringParam(
			params,
			"terminalId",
			method,
		);
		const terminal = this._acpTerminals.get(terminalId);
		if (!terminal) {
			throw new AcpDispatchError(
				-32602,
				`ACP terminal not found: ${terminalId}`,
			);
		}
		return terminal;
	}

	private _appendAcpTerminalOutput(
		terminal: AcpTerminalEntry,
		data: Uint8Array,
	): void {
		const chunk = Buffer.from(data).toString("utf8");
		if (!chunk) {
			return;
		}
		terminal.output += chunk;
		if (
			Number.isFinite(terminal.outputByteLimit) &&
			terminal.outputByteLimit >= 0 &&
			terminal.output.length > terminal.outputByteLimit
		) {
			terminal.output = terminal.output.slice(
				terminal.output.length - terminal.outputByteLimit,
			);
			terminal.truncated = true;
		}
	}

	private async _handleAcpReadFile(
		params: Record<string, unknown>,
	): Promise<{ content: string }> {
		const method = "fs/read";
		const path = this._requireAcpStringParam(params, "path", method);
		const line = this._optionalAcpNumberParam(params, "line", method);
		const limit = this._optionalAcpNumberParam(params, "limit", method);
		const encoding = this._optionalAcpStringParam(params, "encoding", method);
		const bytes = await this.readFile(path);
		if (encoding === "base64") {
			return { content: Buffer.from(bytes).toString("base64") };
		}
		const text = new TextDecoder().decode(bytes);
		if (line === undefined && limit === undefined) {
			return { content: text };
		}
		const startLine = Math.max(1, Math.trunc(line ?? 1));
		const lineLimit =
			limit === undefined
				? Number.POSITIVE_INFINITY
				: Math.max(0, Math.trunc(limit));
		return {
			content: text
				.split("\n")
				.slice(startLine - 1, startLine - 1 + lineLimit)
				.join("\n"),
		};
	}

	private async _handleAcpWriteFile(
		params: Record<string, unknown>,
	): Promise<null> {
		const method = "fs/write";
		const path = this._requireAcpStringParam(params, "path", method);
		const content = this._requireAcpStringParam(params, "content", method);
		const encoding = this._optionalAcpStringParam(params, "encoding", method);
		await this.writeFile(
			path,
			encoding === "base64" ? Buffer.from(content, "base64") : content,
		);
		return null;
	}

	private async _handleAcpReadDir(params: Record<string, unknown>): Promise<{
		entries: Array<{
			name: string;
			path: string;
			type: "file" | "directory" | "symlink";
		}>;
	}> {
		const method = "fs/readDir";
		const path = this._requireAcpStringParam(params, "path", method);
		const entries = await this._vfs().readDirWithTypes(path);
		return {
			entries: entries
				.filter((entry) => entry.name !== "." && entry.name !== "..")
				.map((entry) => ({
					name: entry.name,
					path: path === "/" ? `/${entry.name}` : `${path}/${entry.name}`,
					type: entry.isSymbolicLink
						? "symlink"
						: entry.isDirectory
							? "directory"
							: "file",
				})),
		};
	}

	private _handleAcpCreateTerminal(params: Record<string, unknown>): {
		terminalId: string;
	} {
		const method = "terminal/create";
		const command = this._requireAcpStringParam(params, "command", method);
		const args = this._optionalAcpStringArrayParam(params, "args", method);
		const env = this._optionalAcpEnvParam(params, "env", method);
		const cwd = this._optionalAcpStringParam(params, "cwd", method);
		const cols = this._optionalAcpNumberParam(params, "cols", method);
		const rows = this._optionalAcpNumberParam(params, "rows", method);
		const outputByteLimit = Math.max(
			0,
			Math.trunc(
				this._optionalAcpNumberParam(params, "outputByteLimit", method) ??
					1_048_576,
			),
		);
		const terminalId = `acp-terminal-${++this._acpTerminalCounter}`;
		const terminal: AcpTerminalEntry = {
			handle: this.#kernel.openShell({
				command,
				...(args ? { args } : {}),
				...(env ? { env } : {}),
				...(cwd ? { cwd } : {}),
				...(cols !== undefined ? { cols: Math.trunc(cols) } : {}),
				...(rows !== undefined ? { rows: Math.trunc(rows) } : {}),
				onStderr: (data) => {
					this._appendAcpTerminalOutput(terminal, data);
				},
			}),
			output: "",
			truncated: false,
			outputByteLimit,
			exitCode: null,
			waitPromise: Promise.resolve(0),
		};
		terminal.handle.onData = (data) => {
			this._appendAcpTerminalOutput(terminal, data);
		};
		terminal.waitPromise = terminal.handle.wait().then((exitCode) => {
			terminal.exitCode = exitCode;
			return exitCode;
		});
		this._acpTerminals.set(terminalId, terminal);
		return { terminalId };
	}

	private _handleAcpWriteTerminal(params: Record<string, unknown>): null {
		const method = "terminal/write";
		const terminal = this._requireAcpTerminal(params, method);
		const data = this._requireAcpStringParam(params, "data", method);
		const encoding = this._optionalAcpStringParam(params, "encoding", method);
		terminal.handle.write(
			encoding === "base64" ? Buffer.from(data, "base64") : data,
		);
		return null;
	}

	private _handleAcpReadTerminal(params: Record<string, unknown>): {
		output: string;
		truncated: boolean;
		exitStatus?: { exitCode: number; signal: null };
	} {
		const terminal = this._requireAcpTerminal(params, "terminal/output");
		return {
			output: terminal.output,
			truncated: terminal.truncated,
			...(terminal.exitCode !== null
				? {
						exitStatus: {
							exitCode: terminal.exitCode,
							signal: null,
						},
					}
				: {}),
		};
	}

	private async _handleAcpWaitForTerminalExit(
		params: Record<string, unknown>,
	): Promise<{ exitCode: number; signal: null }> {
		const terminal = this._requireAcpTerminal(params, "terminal/wait_for_exit");
		const exitCode = await terminal.waitPromise;
		return { exitCode, signal: null };
	}

	private _handleAcpKillTerminal(params: Record<string, unknown>): null {
		const method = "terminal/kill";
		const terminal = this._requireAcpTerminal(params, method);
		const signal = this._optionalAcpNumberParam(params, "signal", method) ?? 15;
		terminal.handle.kill(Math.trunc(signal));
		return null;
	}

	private _handleAcpReleaseTerminal(params: Record<string, unknown>): null {
		const method = "terminal/release";
		const terminalId = this._requireAcpStringParam(
			params,
			"terminalId",
			method,
		);
		const terminal = this._acpTerminals.get(terminalId);
		if (!terminal) {
			throw new AcpDispatchError(
				-32602,
				`ACP terminal not found: ${terminalId}`,
			);
		}
		if (terminal.exitCode === null) {
			terminal.handle.kill();
		}
		this._acpTerminals.delete(terminalId);
		return null;
	}

	private _handleAcpResizeTerminal(params: Record<string, unknown>): null {
		const method = "terminal/resize";
		const terminal = this._requireAcpTerminal(params, method);
		const cols = this._optionalAcpNumberParam(params, "cols", method);
		const rows = this._optionalAcpNumberParam(params, "rows", method);
		if (cols === undefined || rows === undefined) {
			throw new AcpDispatchError(
				-32602,
				`${method} requires numeric cols and rows`,
			);
		}
		terminal.handle.resize(Math.trunc(cols), Math.trunc(rows));
		return null;
	}

	private async _handleAcpPermissionCallback(
		sessionId: string,
		permissionId: string,
		params: Record<string, unknown>,
	): Promise<PermissionReply> {
		const session = this._sessions.get(sessionId);
		if (!session) {
			return "reject";
		}

		if (session.permissionHandlers.size === 0) {
			return "reject";
		}

		try {
			return await new Promise<PermissionReply>((resolve, reject) => {
				const timer = setTimeout(() => {
					session.pendingPermissionReplies.delete(permissionId);
					reject(
						new Error(
							`Timed out waiting for permission reply: ${permissionId}`,
						),
					);
				}, 120_000);
				session.pendingPermissionReplies.set(permissionId, {
					resolve,
					reject,
					timer,
				});

				const permissionRequest: PermissionRequest = {
					permissionId,
					description:
						typeof params.description === "string"
							? params.description
							: undefined,
					params,
				};
				for (const handler of session.permissionHandlers) {
					handler(permissionRequest);
				}
			});
		} catch {
			return "reject";
		}
	}

	/**
	 * Resolve an agent config by ID. Package-provided configs take
	 * precedence over the hardcoded AGENT_CONFIGS.
	 */
	private _resolveAgentConfig(agentType: string): AgentConfig | undefined {
		return (
			this._softwareAgentConfigs.get(agentType) ??
			(AGENT_CONFIGS as Record<string, AgentConfig>)[agentType]
		);
	}

	/**
	 * Gracefully destroy a session: cancel any pending work, close the client,
	 * and remove from tracking. Unlike close() which is abrupt, this attempts
	 * a graceful shutdown sequence.
	 */
	async destroySession(sessionId: string): Promise<void> {
		this._requireSession(sessionId);
		try {
			await this.cancelSession(sessionId);
		} catch {
			// Ignore cancellation failures during teardown.
		}
		await this._closeSessionInternal(sessionId);
	}

	// ── Flat session API (ID-based) ───────────────────────────────

	async prompt(sessionId: string, text: string): Promise<PromptResult> {
		const session = this._requireSession(sessionId);
		let agentText = "";
		const handler: SessionEventHandler = (event) => {
			const params = toRecord(event.params);
			const update = toRecord(params.update);
			if (update?.sessionUpdate === "agent_message_chunk") {
				const content = toRecord(update.content);
				if (typeof content.text === "string") {
					agentText += content.text;
				}
			}
		};
		const unsubscribe = this._subscribeSessionEvents(session, handler);

		try {
			const response = await this._sendSessionRequest(
				sessionId,
				"session/prompt",
				{
					prompt: [{ type: "text", text }],
				},
			);
			return { response, text: agentText };
		} finally {
			unsubscribe();
		}
	}

	/** Cancel ongoing agent work for a session. */
	async cancelSession(sessionId: string): Promise<JsonRpcResponse> {
		this._requireSession(sessionId);
		const cancelledPendingPrompt = this._cancelPendingPromptRequests(sessionId);
		if (cancelledPendingPrompt) {
			// Session control requests share the same framed sidecar transport as an
			// in-flight prompt request. If the adapter is blocked in prompt I/O, a
			// synchronous cancel RPC can wedge behind that prompt until the transport
			// timeout fires. Resolve the local prompt immediately, then let the sidecar
			// cancellation continue in the background as best effort.
			void this._sendSessionRequest(sessionId, "session/cancel").catch(
				() => {},
			);
			return {
				jsonrpc: "2.0",
				id: null,
				result: {
					cancelled: true,
					requested: true,
					via: "prompt-fallback",
				},
			};
		}
		const response = await this._sendSessionRequest(
			sessionId,
			"session/cancel",
		);
		if (response.error?.code === -32601) {
			return {
				jsonrpc: "2.0",
				id: null,
				result: {
					cancelled: false,
					requested: true,
					via: "unsupported",
				},
			};
		}
		return response;
	}

	closeSession(sessionId: string): void {
		if (
			!this._sessions.has(sessionId) &&
			!this._closedSessionIds.has(sessionId) &&
			!this._sessionClosePromises.has(sessionId)
		) {
			throw new Error(`Session not found: ${sessionId}`);
		}
		const closePromise = this._closeSessionInternal(sessionId);
		// `closeSession()` is intentionally fire-and-forget; suppress unhandled
		// rejections here and let tracked close promises surface errors to any
		// internal/test callers awaiting `_sessionClosePromises`.
		void closePromise.catch(() => {});
	}

	async respondPermission(
		sessionId: string,
		permissionId: string,
		reply: PermissionReply,
	): Promise<JsonRpcResponse> {
		const session = this._requireSession(sessionId);
		const pendingReply = session.pendingPermissionReplies.get(permissionId);
		if (pendingReply) {
			session.pendingPermissionReplies.delete(permissionId);
			clearTimeout(pendingReply.timer);
			pendingReply.resolve(reply);
			return {
				jsonrpc: "2.0",
				id: null,
				result: {
					permissionId,
					reply,
					via: "sidecar-request",
				},
			};
		}

		return this._sendSessionRequest(sessionId, LEGACY_PERMISSION_METHOD, {
			permissionId,
			reply,
		});
	}

	async setSessionMode(
		sessionId: string,
		modeId: string,
	): Promise<JsonRpcResponse> {
		return this._sendSessionRequest(sessionId, "session/set_mode", {
			modeId,
		});
	}

	getSessionModes(sessionId: string): SessionModeState | null {
		return this._requireSession(sessionId).modes;
	}

	async setSessionModel(
		sessionId: string,
		model: string,
	): Promise<JsonRpcResponse> {
		return this._setSessionConfigByCategory(sessionId, "model", model);
	}

	async setSessionThoughtLevel(
		sessionId: string,
		level: string,
	): Promise<JsonRpcResponse> {
		return this._setSessionConfigByCategory(sessionId, "thought_level", level);
	}

	getSessionConfigOptions(sessionId: string): SessionConfigOption[] {
		return [...this._requireSession(sessionId).configOptions];
	}

	getSessionCapabilities(sessionId: string): AgentCapabilities | null {
		const caps = this._requireSession(sessionId).capabilities;
		return Object.keys(caps).length > 0 ? caps : null;
	}

	getSessionAgentInfo(sessionId: string): AgentInfo | null {
		return this._requireSession(sessionId).agentInfo;
	}

	async rawSessionSend(
		sessionId: string,
		method: string,
		params?: Record<string, unknown>,
	): Promise<JsonRpcResponse> {
		return this._sendSessionRequest(sessionId, method, params);
	}

	async rawSend(
		sessionId: string,
		method: string,
		params?: Record<string, unknown>,
	): Promise<JsonRpcResponse> {
		return this.rawSessionSend(sessionId, method, params);
	}

	onSessionEvent(sessionId: string, handler: SessionEventHandler): () => void {
		const session = this._requireSession(sessionId);
		return this._subscribeSessionEvents(session, handler);
	}

	onPermissionRequest(
		sessionId: string,
		handler: PermissionRequestHandler,
	): () => void {
		const session = this._requireSession(sessionId);
		session.permissionHandlers.add(handler);
		return () => {
			session.permissionHandlers.delete(handler);
		};
	}

	// ── Cron ────────────────────────────────────────────────────

	/** Schedule a cron job. Returns a handle with the job ID and a cancel method. */
	scheduleCron(options: CronJobOptions): CronJob {
		return this._cronManager.schedule(options);
	}

	/** List all registered cron jobs. */
	listCronJobs(): CronJobInfo[] {
		return this._cronManager.list();
	}

	/** Cancel a cron job by ID. */
	cancelCronJob(id: string): void {
		this._cronManager.cancel(id);
	}

	/** Subscribe to cron lifecycle events (fire, complete, error). */
	onCronEvent(handler: CronEventHandler): void {
		this._cronManager.onEvent(handler);
	}

	async dispose(): Promise<void> {
		this._cronManager.dispose();

		for (const sessionId of [...this._sessions.keys()]) {
			await this._closeSessionInternal(sessionId).catch(() => {});
		}

		for (const [id, entry] of this._shells) {
			entry.handle.kill();
		}
		const shellExitPromises = [...this._pendingShellExitPromises];
		this._shells.clear();
		const terminalExitPromises: Promise<unknown>[] = [];
		for (const terminal of this._acpTerminals.values()) {
			terminal.handle.kill();
			terminalExitPromises.push(
				terminal.waitPromise.then(
					() => undefined,
					() => undefined,
				),
			);
		}
		this._acpTerminals.clear();
		this._processes.clear();
		await waitForTrackedExitPromises(
			[...shellExitPromises, ...terminalExitPromises],
			SHELL_DISPOSE_TIMEOUT_MS,
		);

		this._disposeSidecarEventListener();

		const sidecarLease = this._sidecarLease;
		this._sidecarLease = null;
		if (sidecarLease) {
			return sidecarLease.dispose();
		}
		return this.#kernel.dispose();
	}
}

const agentOsRuntimeAdmins = new WeakMap<AgentOs, AgentOsRuntimeAdmin>();

export function getAgentOsRuntimeAdmin(vm: AgentOs): AgentOsRuntimeAdmin {
	const admin = agentOsRuntimeAdmins.get(vm);
	if (!admin) {
		throw new Error("Agent OS runtime admin is not available for this VM");
	}
	return admin;
}

export function getAgentOsKernel(vm: AgentOs): Kernel {
	return getAgentOsRuntimeAdmin(vm).kernel;
}

function resolveAgentOsSidecar(
	config: AgentOsSidecarConfig | undefined,
): AgentOsSidecar {
	if (!config || config.kind === "shared") {
		return getSharedAgentOsSidecarInternal(
			config?.kind === "shared" ? { pool: config.pool } : undefined,
		);
	}

	return config.handle;
}

interface CreateInProcessSidecarTransportOptions<
	TVmAdmin extends InProcessSidecarVmAdmin,
> {
	createVm(
		sessionBootstrap: AgentOsSidecarSessionBootstrap,
		vmBootstrap: AgentOsSidecarVmBootstrap,
	): Promise<TVmAdmin>;
}

interface InProcessSidecarTransport<TVmAdmin extends InProcessSidecarVmAdmin>
	extends AgentOsSidecarTransport {
	getVmAdmin(vmId: string): TVmAdmin | undefined;
}

interface AgentOsSidecarLeaseRecord {
	dispose(): Promise<void>;
}

interface SharedSidecarNativeProcess {
	client: SidecarProcess;
	session: AuthenticatedSession;
}

interface AgentOsSidecarState {
	description: AgentOsSidecarDescription;
	activeLeases: Set<AgentOsSidecarLeaseRecord>;
	sharedPool?: string;
	/**
	 * The single native sidecar process shared by every VM leased from this
	 * handle. Spawned lazily on first VM creation and reused thereafter so VMs
	 * are cheap incremental tenants of one process rather than one-process-each.
	 */
	nativeProcess?: Promise<SharedSidecarNativeProcess>;
}

const sidecarStates = new WeakMap<AgentOsSidecar, AgentOsSidecarState>();
const sharedSidecars = new Map<string, AgentOsSidecar>();

/**
 * Spawn-once accessor for a sidecar handle's shared native process. Concurrent
 * callers await the same promise, so one `AgentOsSidecar` maps to exactly one
 * `agent-os-sidecar` OS process for its whole lifetime.
 */
function ensureSharedSidecarNativeProcess(
	sidecar: AgentOsSidecar,
): Promise<SharedSidecarNativeProcess> {
	const state = getSidecarState(sidecar);
	if (!state.nativeProcess) {
		state.nativeProcess = (async () => {
			const client = SidecarProcess.spawn({
				cwd: REPO_ROOT,
				command: ensureNativeSidecarBinary(),
				args: [],
				frameTimeoutMs: NATIVE_SIDECAR_FRAME_TIMEOUT_MS,
			});
			const session = await client.authenticateAndOpenSession();
			return { client, session };
		})();
	}
	return state.nativeProcess;
}

/** Dispose a sidecar handle's shared native process, if one was spawned. */
async function disposeSharedSidecarNativeProcess(
	state: AgentOsSidecarState,
): Promise<void> {
	const pending = state.nativeProcess;
	if (!pending) {
		return;
	}
	state.nativeProcess = undefined;
	try {
		const { client } = await pending;
		await client.dispose();
	} catch {
		// Process may have already exited; nothing to reclaim.
	}
}

export class AgentOsSidecar {
	constructor(
		sidecarId: string,
		placement: AgentOsSidecarPlacement,
		sharedPool?: string,
	) {
		sidecarStates.set(this, {
			description: {
				sidecarId,
				placement: cloneSidecarPlacement(placement),
				state: "ready",
				activeVmCount: 0,
			},
			activeLeases: new Set(),
			sharedPool,
		});
	}

	describe(): AgentOsSidecarDescription {
		const state = getSidecarState(this);
		return cloneSidecarDescription(state.description);
	}

	async dispose(): Promise<void> {
		const state = getSidecarState(this);
		if (state.description.state === "disposed") {
			return;
		}

		state.description.state = "disposing";
		const errors: Error[] = [];
		for (const lease of [...state.activeLeases]) {
			try {
				await lease.dispose();
			} catch (error) {
				errors.push(error instanceof Error ? error : new Error(String(error)));
			}
		}
		state.activeLeases.clear();
		state.description.activeVmCount = 0;
		// Tear down the shared native process after all leased VMs are gone.
		await disposeSharedSidecarNativeProcess(state);
		state.description.state = "disposed";
		if (state.sharedPool && sharedSidecars.get(state.sharedPool) === this) {
			sharedSidecars.delete(state.sharedPool);
		}
		if (errors.length > 0) {
			throw new Error(errors.map((error) => error.message).join("; "));
		}
	}
}

function createAgentOsSidecarInternal(
	options: AgentOsCreateSidecarOptions = {},
): AgentOsSidecar {
	const sidecarId = options.sidecarId ?? `agentos-sidecar-${randomUUID()}`;
	return new AgentOsSidecar(sidecarId, {
		kind: "explicit",
		sidecarId,
	});
}

/**
 * Test-only escape hatch: dispose every cached shared sidecar so vitest
 * workers can exit cleanly. The shared sidecar is normally process-global and
 * keeps its native subprocess alive across `AgentOs.create()` calls; without
 * this hook the vitest worker can hold open piped stdio handles after the
 * test suite finishes and stall `pnpm test` indefinitely.
 */
export async function __disposeAllSharedSidecarsForTesting(): Promise<void> {
	const sidecars = Array.from(sharedSidecars.values());
	sharedSidecars.clear();
	const errors: Error[] = [];
	for (const sidecar of sidecars) {
		try {
			await sidecar.dispose();
		} catch (error) {
			errors.push(error instanceof Error ? error : new Error(String(error)));
		}
	}
	if (errors.length > 0) {
		throw new Error(
			`failed to dispose shared sidecars: ${errors.map((error) => error.message).join("; ")}`,
		);
	}
}

function getSharedAgentOsSidecarInternal(
	options: AgentOsSharedSidecarOptions = {},
): AgentOsSidecar {
	const pool = options.pool ?? "default";
	const existing = sharedSidecars.get(pool);
	if (existing && existing.describe().state !== "disposed") {
		return existing;
	}

	const sidecar = new AgentOsSidecar(
		`agentos-shared-sidecar:${pool}`,
		{ kind: "shared", ...(pool ? { pool } : {}) },
		pool,
	);
	sharedSidecars.set(pool, sidecar);
	return sidecar;
}

async function leaseAgentOsSidecarVm<TVmAdmin extends InProcessSidecarVmAdmin>(
	sidecar: AgentOsSidecar,
	options: CreateInProcessSidecarTransportOptions<TVmAdmin>,
): Promise<AgentOsSidecarVmLease<TVmAdmin>> {
	const state = getSidecarState(sidecar);
	if (state.description.state !== "ready") {
		throw new Error(
			`Cannot lease VM from sidecar ${state.description.sidecarId} while it is ${state.description.state}`,
		);
	}

	let transport: InProcessSidecarTransport<TVmAdmin> | undefined;
	const client: AgentOsSidecarClient = createAgentOsSidecarClient({
		async createSessionTransport(sessionBootstrap) {
			transport = await createInProcessSidecarTransport(
				sessionBootstrap,
				options,
			);
			return transport;
		},
	});

	let disposed = false;
	let leaseRecord: AgentOsSidecarLeaseRecord | undefined;

	try {
		const session = await client.createSession({
			placement: cloneSidecarPlacement(state.description.placement),
		});
		const vm = await session.createVm();
		const admin = transport?.getVmAdmin(vm.vmId);
		if (!admin) {
			throw new Error(`Sidecar VM admin was not registered for ${vm.vmId}`);
		}

		const lease: AgentOsSidecarVmLease<TVmAdmin> = {
			sidecar,
			session,
			vm,
			admin,
			async dispose() {
				if (disposed) {
					return;
				}
				disposed = true;
				state.activeLeases.delete(leaseRecord!);
				state.description.activeVmCount = state.activeLeases.size;
				await client.dispose();
			},
		};

		leaseRecord = {
			dispose: () => lease.dispose(),
		};
		state.activeLeases.add(leaseRecord);
		state.description.activeVmCount = state.activeLeases.size;
		return lease;
	} catch (error) {
		await client.dispose().catch(() => {});
		throw error;
	}
}

async function createInProcessSidecarTransport<
	TVmAdmin extends InProcessSidecarVmAdmin,
>(
	sessionBootstrap: AgentOsSidecarSessionBootstrap,
	options: CreateInProcessSidecarTransportOptions<TVmAdmin>,
): Promise<InProcessSidecarTransport<TVmAdmin>> {
	const vmAdmins = new Map<string, TVmAdmin>();
	let disposed = false;

	async function disposeVmAdmin(vmId: string): Promise<void> {
		const admin = vmAdmins.get(vmId);
		if (!admin) {
			return;
		}

		vmAdmins.delete(vmId);
		await admin.dispose();
	}

	return {
		async createVm(vmBootstrap) {
			if (disposed) {
				throw new Error(
					`Cannot create VM ${vmBootstrap.vmId} for disposed sidecar session ${sessionBootstrap.sessionId}`,
				);
			}

			const admin = await options.createVm(sessionBootstrap, vmBootstrap);
			vmAdmins.set(vmBootstrap.vmId, admin);
		},

		async disposeVm(vmId) {
			await disposeVmAdmin(vmId);
		},

		async dispose() {
			if (disposed) {
				return;
			}
			disposed = true;

			const errors: Error[] = [];
			for (const vmId of [...vmAdmins.keys()]) {
				try {
					await disposeVmAdmin(vmId);
				} catch (error) {
					errors.push(
						error instanceof Error ? error : new Error(String(error)),
					);
				}
			}

			if (errors.length > 0) {
				throw new Error(errors.map((error) => error.message).join("; "));
			}
		},

		getVmAdmin(vmId) {
			return vmAdmins.get(vmId);
		},
	};
}

function getSidecarState(sidecar: AgentOsSidecar): AgentOsSidecarState {
	const state = sidecarStates.get(sidecar);
	if (!state) {
		throw new Error("Unknown Agent OS sidecar handle");
	}
	return state;
}

function cloneSidecarDescription(
	description: AgentOsSidecarDescription,
): AgentOsSidecarDescription {
	return {
		...description,
		placement: cloneSidecarPlacement(description.placement),
	};
}

function cloneSidecarPlacement(
	placement: AgentOsSidecarPlacement,
): AgentOsSidecarPlacement {
	if (placement.kind === "shared") {
		return {
			kind: "shared",
			...(placement.pool ? { pool: placement.pool } : {}),
		};
	}

	return {
		kind: "explicit",
		sidecarId: placement.sidecarId,
	};
}
