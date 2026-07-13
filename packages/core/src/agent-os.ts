import { randomUUID } from "node:crypto";
import { posix as posixPath } from "node:path";
import type { NativeMountPluginDescriptor } from "@rivet-dev/agentos-runtime-core/descriptors";
import type { CreateVmConfig } from "@rivet-dev/agentos-runtime-core/vm-config";
import type {
	AgentCapabilities,
	AgentInfo,
	PermissionReply,
	PermissionRequest,
	PermissionRequestHandler,
	SessionConfigOption,
	SessionEventHandler,
	SessionModeState,
} from "./agent-session-types.js";
import type { HostTool, ToolKit } from "./host-tools.js";
import { zodToJsonSchema } from "./host-tools-zod.js";
import type { JsonRpcNotification, JsonRpcResponse } from "./json-rpc.js";
import { parseAgentOsOptions } from "./options-schema.js";
import type {
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
} from "./runtime.js";
import { resolvePublishedSidecarBinary } from "./sidecar/binary.js";

export type {
	MountConfigJsonObject,
	MountConfigJsonPrimitive,
	MountConfigJsonValue,
	NativeMountPluginDescriptor,
} from "@rivet-dev/agentos-runtime-core/descriptors";
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

const ACP_EXTENSION_NAMESPACE = "dev.rivet.agent-os.acp";
const SHELL_DISPOSE_TIMEOUT_MS = 5_000;

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
}

/** Entry in the agent registry, describing an available agent type. */
export interface AgentRegistryEntry {
	id: string;
	installed: boolean;
	/** Guest entrypoint the sidecar launches for this agent (`/opt/agentos/bin/<acpEntrypoint>`). */
	adapterEntrypoint: string;
}

import type { PackageRef, SoftwarePackageRef } from "./agentos-package.js";
import { CronManager } from "./cron/cron-manager.js";
import type {
	CronEventHandler,
	CronJob,
	CronJobInfo,
	CronJobOptions,
} from "./cron/types.js";
import { resolveDefaultSoftware } from "./default-software.js";
import type { FilesystemEntry } from "./filesystem-snapshot.js";
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
import type { SoftwareInput } from "./packages.js";
import {
	type AcpRequest,
	type AcpResponse,
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
	NativeSidecarKernelProxy,
	type RootFilesystemEntry,
	type SidecarMountDescriptor,
	SidecarProcess,
	type SidecarRegisteredHostCallbackDefinition,
	type SidecarRequestFrame,
	type SidecarResponsePayload,
	type SidecarSessionState,
	serializeRootFilesystemForSidecar,
} from "./sidecar/rpc-client.js";
import type { AgentType } from "./types.js";

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

interface AgentOsVmAdmin extends InProcessSidecarVmAdmin {
	kernel: Kernel;
	rootView: VirtualFileSystem;
	env: Record<string, string>;
	sidecarMounts: SidecarMountDescriptor[];
	sidecarClient: SidecarProcess;
	sidecarSession: AuthenticatedSession;
	sidecarVm: CreatedVm;
	toolKits: ToolKit[];
}

interface SessionEventSubscriber {
	handler: SessionEventHandler;
}

interface AgentSessionEntry {
	sessionId: string;
	agentType: string;
	processId: string;
	pid: number | null;
	eventHandlers: Set<SessionEventSubscriber>;
	permissionHandlers: Set<PermissionRequestHandler>;
	/**
	 * Set once we have emitted the "no permission handler registered" warning for
	 * this session, so a tool-heavy turn does not re-warn on every request.
	 */
	warnedNoPermissionHandler: boolean;
	pendingPermissionReplies: Map<
		string,
		{
			resolve: (reply: PermissionReply) => void;
			reject: (error: Error) => void;
			timer: ReturnType<typeof setTimeout>;
		}
	>;
}

interface ShellEntry {
	handle: ShellHandle;
	dataHandlers: Set<(data: Uint8Array) => void>;
	exitPromise: Promise<number>;
	closing: boolean;
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
		syncRpcWaitTimeoutMs?: number;
		cpuTimeLimitMs?: number;
		wallClockLimitMs?: number;
		importCacheMaterializeTimeoutMs?: number;
		capturedOutputLimitBytes?: number;
		stdinBufferLimitBytes?: number;
		eventPayloadLimitBytes?: number;
		v8IpcMaxFrameBytes?: number;
	};
	/** Guest Python runtime limits. */
	python?: {
		outputBufferMaxBytes?: number;
		executionTimeoutMs?: number;
		maxOldSpaceMb?: number;
		vfsRpcTimeoutMs?: number;
	};
	/** Guest WASM runtime limits. */
	wasm?: {
		maxModuleFileBytes?: number;
		capturedOutputLimitBytes?: number;
		syncReadLimitBytes?: number;
		prewarmTimeoutMs?: number;
		runnerHeapLimitMb?: number;
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
 * Auto-restart outcome reported on an {@link AgentExitEvent}. Mirrors the
 * sidecar's `AcpAgentExitedEvent.restart` strings:
 * - `"restarted"` — the adapter was respawned and the session was natively
 *   re-attached under the same session id; the session stays usable.
 * - `"unsupported"` — the adapter does not advertise a native resume
 *   capability (`loadSession`/`resume`); the session was evicted.
 * - `"failed"` — the respawn or re-attach errored; the session was evicted.
 * - `"exhausted"` — the per-session restart budget was already spent; evicted.
 */
export type AgentRestartOutcome =
	| "restarted"
	| "unsupported"
	| "failed"
	| "exhausted";

/**
 * An unexpected ACP adapter process exit — a crash from the host's
 * perspective (any spontaneous exit without `closeSession()`, including exit
 * code 0) — plus the sidecar's bounded auto-restart outcome.
 */
export interface AgentExitEvent {
	sessionId: string;
	agentType: string;
	/** Sidecar process id of the adapter that exited. */
	processId: string;
	pid: number | null;
	/** Adapter exit code; `null` when the exit was observed indirectly. */
	exitCode: number | null;
	/** Auto-restart outcome; only `"restarted"` leaves the session usable. */
	restart: AgentRestartOutcome;
	/** Restarts consumed for this session so far. */
	restartCount: number;
	/** Per-session restart budget. */
	maxRestarts: number;
}

export type AgentExitHandler = (event: AgentExitEvent) => void;

function defaultAgentExitHandler(event: AgentExitEvent): void {
	process.stderr.write(
		`[agentos] agent adapter exited unexpectedly: session=${event.sessionId} agent=${event.agentType} exitCode=${event.exitCode ?? "unknown"} restart=${event.restart} (${event.restartCount}/${event.maxRestarts})\n`,
	);
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
	 * Software to install in the VM. Each entry is a package-dir ref. Arrays are
	 * flattened, so meta-packages that export arrays of sub-packages work directly.
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
	/**
	 * Opt in to a high-resolution monotonic guest clock (microsecond class)
	 * for guest Node processes. Default `false` keeps the security-oriented
	 * 1ms timer resolution — untrusted guest code should not get a precise
	 * timer (timing side channels). Enable only for trusted benchmarking or
	 * profiling workloads.
	 */
	highResolutionTime?: boolean;
	/** Root filesystem configuration. Defaults to an overlay with the bundled base snapshot as its deepest lower. */
	rootFilesystem?: RootFilesystemConfig;
	/** Filesystems to mount at boot time. */
	mounts?: MountConfig[];
	/** Additional instructions appended to the base OS system prompt injected at session start. */
	additionalInstructions?: string;
	/** Host-side toolkits available to agents inside the VM. */
	toolKits?: ToolKit[];
	/**
	 * Custom permission policy for the kernel. Controls access to filesystem,
	 * network, child process, and environment operations. When omitted, the
	 * sidecar defaults every category to allow.
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
	 * Called when the ACP adapter process behind a session exits without
	 * `closeSession()` — i.e. an adapter crash. The sidecar auto-restarts the
	 * adapter (bounded per session, natively re-attaching the same session id)
	 * and reports the outcome on the event; only `restart === "restarted"`
	 * leaves the session usable. Defaults to writing a warning line to
	 * `process.stderr`.
	 */
	onAgentExit?: AgentExitHandler;
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

function toRecord(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: {};
}

interface NormalizedPackageRef {
	path: string;
}

function normalizePackageRef(value: unknown): NormalizedPackageRef | undefined {
	// The single package reference is `packagePath`: the packed `.aospkg` file
	// (registry-built packages export `{ packagePath }`), or a package dir for
	// local transition fixtures. A raw string is shorthand for the same path.
	if (typeof value === "string") {
		return { path: value };
	}
	const record = toRecord(value);
	if (typeof record.packagePath === "string") {
		return { path: record.packagePath };
	}
	// Recognizably-legacy shapes fail loudly: silently dropping a software
	// entry boots a VM with missing packages and no diagnostic.
	for (const legacy of ["packageTar", "packageDir", "dir"]) {
		if (typeof record[legacy] === "string") {
			throw new Error(
				`agentOS package ref uses removed field "${legacy}" (value: ${JSON.stringify(record[legacy])}); ` +
					"packages are referenced by a single `packagePath` — update the package " +
					"(rebuild @agentos-software/* dependencies) or pass { packagePath }",
			);
		}
	}
	return undefined;
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

function sessionEntryFromState(state: SidecarSessionState): AgentSessionEntry {
	return {
		sessionId: state.sessionId,
		agentType: state.agentType,
		processId: state.processId,
		pid: state.pid ?? null,
		eventHandlers: new Set(),
		permissionHandlers: new Set(),
		warnedNoPermissionHandler: false,
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

function requireSnapshotField<K extends keyof RootFilesystemEntry>(
	entry: RootFilesystemEntry,
	field: K,
): NonNullable<RootFilesystemEntry[K]> {
	const value = entry[field];
	if (value === undefined || value === null) {
		throw new Error(
			`sidecar root snapshot for ${entry.path} is missing ${String(field)}`,
		);
	}
	return value as NonNullable<RootFilesystemEntry[K]>;
}

function toSnapshotModeString(mode: number): string {
	return `0${(mode & 0o7777).toString(8)}`;
}

function convertSidecarRootSnapshotEntries(
	entries: RootFilesystemEntry[],
): FilesystemEntry[] {
	return entries.map((entry) => {
		const baseEntry: FilesystemEntry = {
			path: entry.path,
			type: entry.kind,
			mode: toSnapshotModeString(requireSnapshotField(entry, "mode")),
			uid: requireSnapshotField(entry, "uid"),
			gid: requireSnapshotField(entry, "gid"),
		};

		if (entry.kind === "file") {
			return {
				...baseEntry,
				content: requireSnapshotField(entry, "content"),
				encoding: requireSnapshotField(entry, "encoding"),
			};
		}

		if (entry.kind === "symlink") {
			return {
				...baseEntry,
				target: requireSnapshotField(entry, "target"),
			};
		}

		return baseEntry;
	});
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
				readOnly: mount.readOnly,
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
			readOnly:
				mount.filesystem.mode === undefined ? undefined : mode === "read-only",
		});
	}

	return resolved;
}

function collectSidecarMountPlan(options: { mounts?: MountConfig[] }): {
	sidecarMounts: Array<ReturnType<typeof serializeMountConfigForSidecar>>;
} {
	const sidecarMounts: Array<
		ReturnType<typeof serializeMountConfigForSidecar>
	> = [];
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
	}

	for (const mount of options.mounts ?? []) {
		if (!isNativeMountConfig(mount)) {
			const readOnly = isOverlayMountConfig(mount)
				? mount.filesystem.mode === undefined
					? undefined
					: mount.filesystem.mode === "read-only"
				: mount.readOnly;
			sidecarMounts.push({
				guestPath: mount.path,
				...(readOnly === undefined ? {} : { readOnly }),
				plugin: {
					id: "js_bridge",
				},
			});
			continue;
		}
		pushMount(mount);
	}

	return { sidecarMounts };
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

interface VmFetchResponsePayload {
	status: number;
	statusText: string;
	headers: Array<[string, string]>;
	body: string;
}

function requireVmFetchResponsePayload(value: unknown): VmFetchResponsePayload {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new Error("sidecar vm.fetch response must be an object");
	}
	const payload = value as Record<string, unknown>;
	if (!Number.isInteger(payload.status)) {
		throw new Error("sidecar vm.fetch response is missing numeric status");
	}
	if (typeof payload.statusText !== "string") {
		throw new Error("sidecar vm.fetch response is missing statusText");
	}
	if (
		!Array.isArray(payload.headers) ||
		payload.headers.some(
			(entry) =>
				!Array.isArray(entry) ||
				entry.length !== 2 ||
				typeof entry[0] !== "string" ||
				typeof entry[1] !== "string",
		)
	) {
		throw new Error("sidecar vm.fetch response is missing valid headers");
	}
	if (typeof payload.body !== "string") {
		throw new Error("sidecar vm.fetch response is missing base64 body");
	}
	return payload as unknown as VmFetchResponsePayload;
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

	const tool = context.toolMap.get(payload.callback_key);
	if (!tool) {
		return {
			type: "host_callback_result",
			invocation_id: payload.invocation_id,
			error: `Unknown tool "${payload.callback_key}"`,
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
			result: await executeHostTool(tool, parsed.data),
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

interface HostCallbackContext {
	toolMap: ReadonlyMap<string, HostTool>;
}

interface JsBridgeContext {
	resolveTarget(
		mountId: string,
	): { filesystem: VirtualFileSystem; rootPath: string } | undefined;
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
		const target = context.resolveTarget(request.mount_id);
		if (!target) {
			throw new Error(`Unknown js_bridge mount id: ${request.mount_id}`);
		}
		const fs = target.filesystem;
		const path = () => bridgePath(target.rootPath, args.path);
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
					bridgePath(target.rootPath, args.oldPath),
					bridgePath(target.rootPath, args.newPath),
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
					bridgePath(target.rootPath, args.linkPath),
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
					bridgePath(target.rootPath, args.oldPath),
					bridgePath(target.rootPath, args.newPath),
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

async function executeHostTool(
	tool: HostTool,
	input: unknown,
): Promise<unknown> {
	const parsed = tool.inputSchema.safeParse(input);
	if (!parsed.success) {
		throw new Error(validationMessage(parsed.error));
	}

	return tool.execute(parsed.data);
}

function serializeToolkitsForSidecar(toolKits: ToolKit[]): Array<{
	name: string;
	description: string;
	callbacks: Record<string, SidecarRegisteredHostCallbackDefinition>;
}> {
	return toolKits.map((toolKit) => {
		return {
			name: toolKit.name,
			description: toolKit.description,
			callbacks: Object.fromEntries(
				Object.entries(toolKit.tools).map(([toolName, tool]) => [
					toolName,
					toolToSidecarDefinition(tool),
				]),
			),
		};
	});
}

export class AgentOs {
	#kernel: Kernel;
	readonly sidecar: AgentOsSidecar;
	private _sessions = new Map<string, AgentSessionEntry>();
	private _processes = new Map<
		number,
		{
			proc: ManagedProcess;
			stdoutHandlers: Set<(data: Uint8Array) => void>;
			stderrHandlers: Set<(data: Uint8Array) => void>;
			exitHandlers: Set<(exitCode: number) => void>;
		}
	>();
	private _shells = new Map<string, ShellEntry>();
	private _pendingShellExitPromises = new Map<string, Promise<number>>();
	private _cronManager!: CronManager;
	private _toolKits: ToolKit[] = [];
	private _sidecarLease: AgentOsSidecarVmLease<AgentOsVmAdmin> | null = null;
	private readonly _sidecarClient: SidecarProcess;
	private readonly _sidecarSession: AuthenticatedSession;
	private readonly _sidecarVm: CreatedVm;
	private readonly _disposeSidecarEventListener: () => void;
	private readonly _agentStderrHandler?: AgentStderrHandler;
	private readonly _agentExitHandler?: AgentExitHandler;
	private readonly _limitWarningHandler?: LimitWarningHandler;

	private constructor(
		kernel: Kernel,
		sidecar: AgentOsSidecar,
		env: Record<string, string>,
		rootFilesystem: VirtualFileSystem,
		sidecarClient: SidecarProcess,
		sidecarSession: AuthenticatedSession,
		sidecarVm: CreatedVm,
		agentStderrHandler?: AgentStderrHandler,
		agentExitHandler?: AgentExitHandler,
		limitWarningHandler?: LimitWarningHandler,
	) {
		this.#kernel = kernel;
		this.sidecar = sidecar;
		this._sidecarClient = sidecarClient;
		this._sidecarSession = sidecarSession;
		this._sidecarVm = sidecarVm;
		this._agentStderrHandler = agentStderrHandler;
		this._agentExitHandler = agentExitHandler;
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
		// Default software is FULLY DYNAMIC: this package's own NON-agent
		// @agentos-software/* dependencies (e.g. common), each default-exporting
		// its registry-built descriptor. Agent packages are NOT projected here;
		// callers or an upper package-manager layer must pass those packages in
		// `software` before createSession(id). Unbuilt packages throw with build
		// instructions; opt out via defaultSoftware: false.
		const defaultSoftware =
			options?.defaultSoftware === false ? [] : resolveDefaultSoftware();
		const software: unknown[] =
			options?.defaultSoftware === false
				? (options.software ?? [])
				: [...defaultSoftware, ...(options?.software ?? [])];
		// Packages are projected by the SIDECAR: the client forwards only the
		// package `path` over `initializeVm` and the sidecar reads metadata from
		// the packed vbare manifest (chunk1 of the `.aospkg`).
		const flatSoftware = software.flat();
		// Honor the AgentOsOptions.defaultSoftware contract ("entries already present
		// in `software` are not duplicated"): the default bundle and an explicitly
		// passed one resolve to the same package paths, so dedup by path. Without
		// this the sidecar rejects the second projection with a duplicate-command
		// error (e.g. coreutils' `[`).
		const seenPackagePaths = new Set<string>();
		const sidecarPackages = flatSoftware.flatMap((entry) => {
			const ref = normalizePackageRef(entry);
			if (!ref || seenPackagePaths.has(ref.path)) {
				return [];
			}
			seenPackagePaths.add(ref.path);
			return [{ path: ref.path }];
		});
		// All package software is projected into `/opt/agentos` by the sidecar. The
		// client stages nothing host-side and parses NO package manifests: the
		// sidecar owns agent resolution, agent enumeration, and agent snapshot
		// bundle loading from the projected package dirs.
		const localMounts = await resolveCompatLocalMounts(options?.mounts);
		const toolKits = options?.toolKits;

		// Resolve the sidecar handle up front so every VM created here leases the
		// one shared native sidecar process owned by that handle.
		const sidecar = resolveAgentOsSidecar(options?.sidecar);

		const createVmAdmin = async (): Promise<AgentOsVmAdmin> => {
			// The `/opt/agentos` projection is built by the sidecar from the
			// forwarded `packages` (it owns the staging dir + read-only mount, and
			// runtime `linkSoftware` appends to that live dir). The client no longer
			// stages packages host-side.
			let rootBridge: NativeSidecarKernelProxy | null = null;
			let kernel: Kernel | null = null;
			let client: SidecarProcess | null = null;
			let createdNativeVm: CreatedVm | null = null;
			let nativeSession: AuthenticatedSession | null = null;
			let cleanedUp = false;

			const cleanup = async (): Promise<void> => {
				if (cleanedUp) {
					return;
				}
				cleanedUp = true;
			};

			try {
				let env: Record<string, string> = {};
				// Guest command paths. The sidecar owns the `/opt/agentos` projection and
				// reports the exact projected package commands after initialization.
				const commandGuestPaths = new Map<string, string>();
				const { sidecarMounts } = collectSidecarMountPlan({
					mounts: options?.mounts,
				});
				// Reuse the sidecar handle's single shared native process; this VM
				// becomes another tenant of it rather than spawning its own process.
				const shared = await ensureSharedSidecarNativeProcess(sidecar);
				client = shared.client;
				const session = shared.session;
				nativeSession = session;
				const sidecarPermissions = options?.permissions
					? serializePermissionsForSidecar(options.permissions)
					: undefined;
				const createVmConfig: CreateVmConfig = {
					...(options?.rootFilesystem !== undefined
						? {
								rootFilesystem: serializeRootFilesystemForSidecar(
									options.rootFilesystem,
								),
							}
						: {}),
					permissions: sidecarPermissions,
					limits: options?.limits,
					...(options?.loopbackExemptPorts !== undefined
						? { loopbackExemptPorts: options.loopbackExemptPorts }
						: {}),
					// 0.3: the Node builtin allow-list moved from configureVm to
					// VM creation. `undefined` => engine default allow-list;
					// `[]` => deny all; `[..]` => exactly those. Platform and
					// module resolution keep their engine defaults (full Node
					// emulation), matching the prior behavior where Agent OS only
					// constrained the builtin allow-list.
					...(options?.allowedNodeBuiltins !== undefined ||
					options?.highResolutionTime !== undefined
						? {
								jsRuntime: {
									...(options?.allowedNodeBuiltins !== undefined
										? { allowedBuiltins: options.allowedNodeBuiltins }
										: {}),
									...(options?.highResolutionTime !== undefined
										? { highResolutionTime: options.highResolutionTime }
										: {}),
								},
							}
						: {}),
					...(options?.additionalInstructions === undefined
						? {}
						: {
								agentAdditionalInstructions: options.additionalInstructions,
							}),
				};
				const nativeVm = await client.initializeVm(session, {
					runtime: "java_script",
					config: createVmConfig,
					...(sidecarMounts.length > 0 ? { mounts: sidecarMounts } : {}),
					...(sidecarPackages.length > 0 ? { packages: sidecarPackages } : {}),
					...(toolKits === undefined || toolKits.length === 0
						? {}
						: { hostCallbacks: serializeToolkitsForSidecar(toolKits) }),
				});
				env = { ...nativeVm.guestEnv };
				createdNativeVm = nativeVm;
				for (const command of nativeVm.projectedCommands) {
					commandGuestPaths.set(command.name, command.guestPath);
				}

				rootBridge = new NativeSidecarKernelProxy({
					client,
					session,
					vm: nativeVm,
					env: nativeVm.guestEnv,
					cwd: nativeVm.guestCwd,
					localMounts,
					sidecarMounts,
					commandGuestPaths,
					onDispose: cleanup,
					// The native process is owned by the AgentOsSidecar handle and
					// shared across VMs; disposing this VM must not kill the process.
					ownsClient: false,
				});
				kernel = rootBridge as unknown as Kernel;
				return {
					env,
					kernel,
					rootView: rootBridge.createRootView(),
					sidecarMounts,
					sidecarClient: client,
					sidecarSession: session,
					sidecarVm: nativeVm,
					toolKits: toolKits ?? [],
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
					await kernel.dispose().catch((cleanupError) => {
						console.warn(
							"failed to dispose kernel after VM startup failure",
							cleanupError,
						);
					});
				}
				if (rootBridge) {
					await rootBridge.dispose().catch((cleanupError) => {
						console.warn(
							"failed to dispose root bridge after VM startup failure",
							cleanupError,
						);
					});
				} else {
					if (createdNativeVm && nativeSession && client) {
						await client
							.disposeVm(nativeSession, createdNativeVm)
							.catch((cleanupError) => {
								console.warn(
									"failed to dispose sidecar VM after startup failure",
									cleanupError,
								);
							});
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
				vmAdmin.env,
				vmAdmin.rootView,
				vmAdmin.sidecarClient,
				vmAdmin.sidecarSession,
				vmAdmin.sidecarVm,
				options?.onAgentStderr ?? defaultAgentStderrHandler,
				options?.onAgentExit ?? defaultAgentExitHandler,
				options?.onLimitWarning,
			);
			vm._sidecarLease = sidecarLease;
			vm._toolKits = vmAdmin.toolKits;
			vm._installSidecarRequestHandler();
			vm._cronManager = new CronManager(
				vmAdmin.sidecarClient,
				vmAdmin.sidecarSession,
				vmAdmin.sidecarVm,
			);

			return vm;
		} catch (error) {
			await sidecarLease?.dispose().catch((cleanupError) => {
				console.warn(
					"failed to dispose sidecar lease after AgentOs.create failure",
					cleanupError,
				);
			});
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
		stdoutHandlers: Set<(data: Uint8Array) => void>,
		stderrHandlers: Set<(data: Uint8Array) => void>,
		exitHandlers: Set<(exitCode: number) => void>,
	): { pid: number } {
		const entry = {
			proc,
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
		void proc
			.wait()
			.then((code) => {
				for (const h of exitHandlers) h(code);
			})
			.catch((error) => {
				console.error(`[agent-os] process ${proc.pid} wait failed`, error);
			});

		return { pid: proc.pid };
	}

	async spawn(
		command: string,
		args: string[],
		options?: KernelSpawnOptions,
	): Promise<{ pid: number }> {
		const stdoutHandlers = new Set<(data: Uint8Array) => void>();
		const stderrHandlers = new Set<(data: Uint8Array) => void>();
		const exitHandlers = new Set<(exitCode: number) => void>();

		// Include caller-provided callbacks in the initial handler sets.
		if (options?.onStdout) stdoutHandlers.add(options.onStdout);
		if (options?.onStderr) stderrHandlers.add(options.onStderr);

		const proc = await this.#kernel.spawn(command, args, {
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
			stdoutHandlers,
			stderrHandlers,
			exitHandlers,
		);
	}

	/** Write data to a process's stdin. */
	writeProcessStdin(pid: number, data: string | Uint8Array): Promise<void> {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		return entry.proc.writeStdin(data);
	}

	/** Close a process's stdin stream. */
	closeProcessStdin(pid: number): Promise<void> {
		const entry = this._processes.get(pid);
		if (!entry) throw new Error(`Process not found: ${pid}`);
		return entry.proc.closeStdin();
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

	async readFile(path: string): Promise<Uint8Array> {
		return this.#kernel.readFile(path);
	}

	async writeFile(path: string, content: string | Uint8Array): Promise<void> {
		return this.#kernel.writeFile(path, content);
	}

	async mkdir(path: string, options?: { recursive?: boolean }): Promise<void> {
		return this.#kernel.mkdir(path, options);
	}

	async readdir(path: string): Promise<string[]> {
		return this.#kernel.readdir(path);
	}

	async readdirRecursive(
		path: string,
		options?: ReaddirRecursiveOptions,
	): Promise<DirEntry[]> {
		const entries = await this.#kernel.readdirRecursive(path, {
			maxDepth: options?.maxDepth,
		});
		return entries.map((entry) => ({
			path: entry.path,
			type: entry.isSymbolicLink
				? "symlink"
				: entry.isDirectory
					? "directory"
					: "file",
			size: entry.size,
		}));
	}

	async stat(path: string): Promise<VirtualStat> {
		return this.#kernel.stat(path);
	}

	async exists(path: string): Promise<boolean> {
		return this.#kernel.exists(path);
	}

	async snapshotRootFilesystem(): Promise<RootSnapshotExport> {
		return createSnapshotExport(
			convertSidecarRootSnapshotEntries(
				await this._sidecarClient.snapshotRootFilesystem(
					this._sidecarSession,
					this._sidecarVm,
				),
			),
		);
	}

	/**
	 * Mount a filesystem into the running VM. Resolves once the mount has been
	 * delivered to the native sidecar, so guest code can use it immediately
	 * after the returned promise settles; a delivery failure rejects instead of
	 * leaving the mount silently host-only.
	 */
	async mountFs(
		path: string,
		driver: VirtualFileSystem,
		options?: { readOnly?: boolean },
	): Promise<void> {
		await this.#kernel.mountFs(path, driver, { readOnly: options?.readOnly });
	}

	async unmountFs(path: string): Promise<void> {
		await this.#kernel.unmountFs(path);
	}

	async move(from: string, to: string): Promise<void> {
		await this.#kernel.movePath(from, to);
	}

	async delete(path: string, options?: { recursive?: boolean }): Promise<void> {
		await this.#kernel.removePath(path, options);
	}

	async fetch(port: number, request: Request): Promise<Response> {
		const url = new URL(request.url);
		const responsePayload = requireVmFetchResponsePayload(
			JSON.parse(
				await this._sidecarClient.vmFetch(
					this._sidecarSession,
					this._sidecarVm,
					{
						port,
						method: request.method,
						path: `${url.pathname}${url.search}`,
						headersJson: JSON.stringify(
							Object.fromEntries(request.headers.entries()),
						),
						...(request.method !== "GET" && request.method !== "HEAD"
							? { body: await request.text() }
							: {}),
					},
				),
			),
		);
		const headers = new Headers();
		for (const [key, value] of responsePayload.headers) {
			headers.append(key, value);
		}
		return new Response(Buffer.from(responsePayload.body, "base64"), {
			status: responsePayload.status,
			statusText: responsePayload.statusText,
			headers,
		});
	}

	async openShell(options?: OpenShellOptions): Promise<{ shellId: string }> {
		const dataHandlers = new Set<(data: Uint8Array) => void>();

		const handle = await this.#kernel.openShell(options);
		const shellId = handle.processId;
		handle.onData = (data) => {
			for (const h of dataHandlers) h(data);
		};

		const entry: ShellEntry = {
			handle,
			dataHandlers,
			exitPromise: Promise.resolve(0),
			closing: false,
		};
		const exitPromise = handle.wait();
		const finalize = () => {
			this._pendingShellExitPromises.delete(shellId);
			if (this._shells.get(shellId) === entry) {
				this._shells.delete(shellId);
			}
		};
		entry.exitPromise = exitPromise.then(
			(exitCode) => {
				finalize();
				return exitCode;
			},
			(error) => {
				finalize();
				throw error;
			},
		);
		this._pendingShellExitPromises.set(shellId, entry.exitPromise);
		this._shells.set(shellId, entry);
		return { shellId };
	}

	/** Write data to a shell's PTY input. */
	writeShell(shellId: string, data: string | Uint8Array): Promise<void> {
		const entry = this._shells.get(shellId);
		if (!entry || entry.closing) throw new Error(`Shell not found: ${shellId}`);
		return entry.handle.write(data);
	}

	/** Subscribe to data output from a shell. Returns an unsubscribe function. */
	onShellData(
		shellId: string,
		handler: (data: Uint8Array) => void,
	): () => void {
		const entry = this._shells.get(shellId);
		if (!entry || entry.closing) throw new Error(`Shell not found: ${shellId}`);
		entry.dataHandlers.add(handler);
		return () => {
			entry.dataHandlers.delete(handler);
		};
	}

	/** Notify a shell of terminal resize and await the sidecar response. */
	async resizeShell(
		shellId: string,
		cols: number,
		rows: number,
	): Promise<void> {
		const entry = this._shells.get(shellId);
		if (!entry || entry.closing) throw new Error(`Shell not found: ${shellId}`);
		await entry.handle.resize(cols, rows);
	}

	/**
	 * Wait for a shell to exit and return its sidecar-authoritative exit code.
	 * A late wait reads the retained sidecar process snapshot.
	 */
	async waitShell(shellId: string): Promise<number> {
		const entry = this._shells.get(shellId);
		if (entry) {
			return entry.exitPromise;
		}
		const pending = this._pendingShellExitPromises.get(shellId);
		if (pending) {
			return pending;
		}
		if (this.#kernel instanceof NativeSidecarKernelProxy) {
			const process = await this.#kernel.processSnapshotById(shellId);
			if (process?.status === "exited" && process.exitCode !== null) {
				return process.exitCode;
			}
		}
		throw new Error(`Shell not found: ${shellId}`);
	}

	/** Kill a shell process, await the sidecar response, and remove it from tracking. */
	async closeShell(shellId: string): Promise<void> {
		const entry = this._shells.get(shellId);
		if (!entry) {
			if (this.#kernel instanceof NativeSidecarKernelProxy) {
				const process = await this.#kernel.processSnapshotById(shellId);
				if (process?.status === "exited") {
					return;
				}
			}
			throw new Error(`Shell not found: ${shellId}`);
		}
		if (entry.closing) {
			return;
		}
		await entry.handle.kill();
		entry.closing = true;
	}

	/** Returns sidecar-authoritative info for processes spawned via spawn(). */
	async listProcesses(): Promise<SpawnedProcessInfo[]> {
		const processByPid = new Map(
			(await this.allProcesses()).map((process) => [process.pid, process]),
		);
		return [...this._processes.keys()].map((pid) => {
			const process = processByPid.get(pid);
			if (!process) {
				throw new Error(
					`Sidecar process snapshot is missing tracked process: ${pid}`,
				);
			}
			return {
				pid: process.pid,
				command: process.command,
				args: process.args.slice(1),
				running: process.status !== "exited",
				exitCode: process.exitCode,
			};
		});
	}

	/** Returns all kernel processes across all active runtimes (WASM and Node). */
	async allProcesses(): Promise<KernelProcessInfo[]> {
		if (this.#kernel instanceof NativeSidecarKernelProxy) {
			return await this.#kernel.snapshotProcesses();
		}
		return [...this.#kernel.processes.values()];
	}

	/** Returns processes organized as a tree using ppid relationships. */
	async processTree(): Promise<ProcessTreeNode[]> {
		const all = await this.allProcesses();
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
	async getProcess(pid: number): Promise<SpawnedProcessInfo> {
		if (!this._processes.has(pid)) {
			throw new Error(`Process not found: ${pid}`);
		}
		const process = (await this.allProcesses()).find(
			(candidate) => candidate.pid === pid,
		);
		if (!process) {
			throw new Error(
				`Sidecar process snapshot is missing tracked process: ${pid}`,
			);
		}
		return {
			pid: process.pid,
			command: process.command,
			args: process.args.slice(1),
			running: process.status !== "exited",
			exitCode: process.exitCode,
		};
	}

	/** Send SIGTERM to gracefully stop a process. No-op if already exited. */
	async stopProcess(pid: number): Promise<void> {
		const entry = this._processes.get(pid);
		if (!entry) {
			throw new Error(`Process not found: ${pid}`);
		}
		await entry.proc.kill();
	}

	/** Send SIGKILL to force-kill a process. No-op if already exited. */
	async killProcess(pid: number): Promise<void> {
		const entry = this._processes.get(pid);
		if (!entry) {
			throw new Error(`Process not found: ${pid}`);
		}
		await entry.proc.kill(9);
	}

	/** Returns the sidecar's authoritative active-session list. */
	async listSessions(): Promise<SessionInfo[]> {
		const response = await this._sendAcpRequest({
			tag: "AcpListSessionsRequest",
			val: { reserved: false },
		});
		if (response.tag !== "AcpListSessionsResponse") {
			throw new Error(
				`unexpected response to AcpListSessionsRequest: ${response.tag}`,
			);
		}
		return response.val.sessions.map((session) => ({ ...session }));
	}

	/** Internal helper: retrieve a session or throw. */
	private _requireSession(sessionId: string): AgentSessionEntry {
		const session = this._sessions.get(sessionId);
		if (!session) {
			throw new Error(`Session not found: ${sessionId}`);
		}
		return session;
	}

	/**
	 * Dynamically link a software package into the RUNNING VM. The package's
	 * `bin/` commands appear under `/opt/agentos/bin` (on `$PATH`) and its `share/man`
	 * pages under MANPATH immediately — the `/opt/agentos` mount is host-backed, so
	 * writing into its staging dir is reflected live with no reboot. An `agent`
	 * block registers the package for `createSession(name)`. Persists for the VM's
	 * lifetime (and across a snapshot iff the volume persists).
	 */
	async linkSoftware(
		descriptor: PackageRef | SoftwarePackageRef,
	): Promise<void> {
		const ref = normalizePackageRef(descriptor);
		if (!ref) {
			throw new Error("Invalid agentOS package reference");
		}
		// Forward to the sidecar, which owns the `/opt/agentos` projection and
		// appends the package to its live host-backed staging dir; the commands
		// appear under `/opt/agentos/bin` immediately. The sidecar rejects a
		// duplicate command, surfaced here as a thrown error.
		const commands = await this._sidecarClient.linkPackage(
			this._sidecarSession,
			this._sidecarVm,
			{ path: ref.path },
		);
		if (this.#kernel instanceof NativeSidecarKernelProxy) {
			this.#kernel.registerCommandGuestPaths(
				new Map(
					commands.projectedCommands.map((command) => [
						command.name,
						command.guestPath,
					]),
				),
			);
		}
		// The client parses no manifests: an `agent` block in the linked package is
		// picked up by the sidecar (it owns the projected `/opt/agentos` and answers
		// createSession/listAgents from it). Nothing to record client-side.
	}

	async providedCommands(): Promise<
		{ packageName: string; commands: string[] }[]
	> {
		return this._sidecarClient.providedCommands(
			this._sidecarSession,
			this._sidecarVm,
		);
	}

	/**
	 * Returns all registered agents with their installation status. Thin forwarder:
	 * sends `AcpListAgentsRequest` and maps the response. The sidecar enumerates the
	 * projected `/opt/agentos` packages (the client parses no manifests). Every such
	 * agent is a package materialized into the VM, so `installed` is always `true`.
	 */
	async listAgents(): Promise<AgentRegistryEntry[]> {
		const response = await this._sendAcpRequest({
			tag: "AcpListAgentsRequest",
			val: { reserved: false },
		});
		if (response.tag !== "AcpListAgentsResponse") {
			throw new Error(`unexpected list_agents response: ${response.tag}`);
		}
		return response.val.agents.map((agent) => ({
			id: agent.id,
			installed: agent.installed,
			adapterEntrypoint: agent.adapterEntrypoint,
		}));
	}

	private _recordSessionNotification(
		session: AgentSessionEntry,
		notification: JsonRpcNotification,
	): void {
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
			} catch (error) {
				console.warn(
					`ACP session event subscriber failed for ${session.sessionId}`,
					error,
				);
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

	/**
	 * Warn once per session (host-visible) that a tool-permission request was
	 * auto-denied because no `onPermissionRequest` handler is registered. Shared
	 * by both the bare-callback and JSON-RPC permission paths so the message and
	 * the once-per-session guard cannot drift between them.
	 */
	private _warnNoPermissionHandlerOnce(
		session: AgentSessionEntry,
		params: Record<string, unknown>,
	): void {
		if (session.warnedNoPermissionHandler) {
			return;
		}
		session.warnedNoPermissionHandler = true;
		this._emitSessionWarning(
			session,
			`agentos: a tool-permission request (${this._permissionToolLabel(params)}) was ` +
				`auto-denied because no onPermissionRequest handler is registered for session ` +
				`${session.sessionId}. Register one with vm.onPermissionRequest(sessionId, ...) and ` +
				`reply via vm.respondPermission(...) to let the agent use tools.`,
		);
	}

	/** Best-effort human label for the tool named in a permission request. */
	private _permissionToolLabel(params: Record<string, unknown>): string {
		if (typeof params.toolName === "string") {
			return params.toolName;
		}
		const toolCall = params.toolCall;
		if (
			toolCall &&
			typeof toolCall === "object" &&
			typeof (toolCall as { title?: unknown }).title === "string"
		) {
			return (toolCall as { title: string }).title;
		}
		return "a tool";
	}

	/**
	 * Emit a host-visible warning for a session through the same agent-process log
	 * channel that surfaces adapter stderr (`onAgentStderr`, default: process
	 * stderr). Used for agent-os-owned diagnostics — e.g. a permission request
	 * that was auto-denied because no host hook is registered — so they never fire
	 * silently inside the sidecar.
	 */
	private _emitSessionWarning(
		session: AgentSessionEntry,
		message: string,
	): void {
		const handler = this._agentStderrHandler;
		if (!handler) {
			return;
		}
		try {
			handler({
				sessionId: session.sessionId,
				agentType: session.agentType,
				processId: session.processId,
				pid: session.pid,
				chunk: new TextEncoder().encode(`${message}\n`),
			});
		} catch (error) {
			console.warn(
				`ACP warning handler failed for ${session.sessionId}`,
				error,
			);
		}
	}

	private _recordAgentStderr(event: {
		sessionId: string;
		agentType: string;
		processId: string;
		chunk: ArrayBuffer;
	}): void {
		const session = this._sessions.get(event.sessionId);
		const handler = this._agentStderrHandler;
		if (!handler) {
			return;
		}
		try {
			handler({
				sessionId: event.sessionId,
				agentType: event.agentType,
				processId: event.processId,
				pid: session?.pid ?? null,
				chunk: new Uint8Array(event.chunk),
			});
		} catch (error) {
			console.warn(`ACP stderr handler failed for ${event.sessionId}`, error);
		}
	}

	private _recordAgentExit(event: {
		sessionId: string;
		agentType: string;
		processId: string;
		exitCode: number | null;
		restart: string;
		restartCount: number;
		maxRestarts: number;
	}): void {
		const session = this._sessions.get(event.sessionId);
		const handler = this._agentExitHandler;
		if (!handler) {
			return;
		}
		try {
			handler({
				sessionId: event.sessionId,
				agentType: event.agentType,
				processId: event.processId,
				pid: session?.pid ?? null,
				exitCode: event.exitCode,
				restart: event.restart as AgentRestartOutcome,
				restartCount: event.restartCount,
				maxRestarts: event.maxRestarts,
			});
		} catch (error) {
			console.warn(`ACP exit handler failed for ${event.sessionId}`, error);
		}
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
		if (event.payload.type === "cron_dispatch") {
			const dispatch = event.payload.dispatch;
			this._cronManager.consumeDispatch({
				alarm: {
					generation: dispatch.alarm.generation,
					...(dispatch.alarm.next_alarm_ms === undefined
						? {}
						: { nextAlarmMs: dispatch.alarm.next_alarm_ms }),
				},
				runs: dispatch.runs.map((run) => ({
					runId: run.run_id,
					jobId: run.job_id,
					action: run.action,
				})),
				events: dispatch.events.map((record) => ({
					kind: record.kind,
					jobId: record.job_id,
					timeMs: record.time_ms,
					...(record.duration_ms === undefined
						? {}
						: { durationMs: record.duration_ms }),
					...(record.error === undefined ? {} : { error: record.error }),
				})),
			});
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
		} catch (error) {
			console.warn("invalid ACP session event from sidecar", error);
		}
	}

	private _handleLimitWarning(detail: Record<string, string>): void {
		if (!this._limitWarningHandler) {
			return;
		}
		let warning: LimitWarning;
		try {
			const requireString = (name: string): string => {
				const value = detail[name];
				if (value === undefined) {
					throw new Error(`missing ${name}`);
				}
				return value;
			};
			const requireNumber = (name: string): number => {
				const value = Number(requireString(name));
				if (!Number.isFinite(value)) {
					throw new Error(`invalid ${name}`);
				}
				return value;
			};
			warning = {
				limit: requireString("limit"),
				category: requireString("category"),
				observed: requireNumber("observed"),
				capacity: requireNumber("capacity"),
				fillPercent: requireNumber("fillPercent"),
			};
		} catch (error) {
			console.warn("invalid limit warning from sidecar", error);
			return;
		}
		try {
			this._limitWarningHandler(warning);
		} catch (error) {
			console.warn("limit warning handler failed", error);
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
				case "AcpAgentExitedEvent": {
					this._recordAgentExit(event.val);
					return;
				}
			}
		} catch (error) {
			console.warn("invalid ACP extension event from sidecar", error);
		}
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
		return (await this._sendSessionRequestWithText(sessionId, method, params))
			.response;
	}

	private async _sendSessionRequestWithText(
		sessionId: string,
		method: string,
		params?: Record<string, unknown>,
	): Promise<{ response: JsonRpcResponse; text: string | null }> {
		const acpResponse = await this._sendAcpRequest({
			tag: "AcpSessionRequest",
			val: {
				sessionId,
				method,
				params: params === undefined ? null : JSON.stringify(params),
			},
		});
		if (acpResponse.tag !== "AcpSessionRpcResponse") {
			throw new Error(
				`unexpected response to AcpSessionRequest: ${acpResponse.tag}`,
			);
		}
		return {
			response: toJsonRpcResponse(JSON.parse(acpResponse.val.response)),
			text: acpResponse.val.text,
		};
	}

	private async _setSessionConfigByCategory(
		sessionId: string,
		category: string,
		value: string,
	): Promise<JsonRpcResponse> {
		const response = await this._sendAcpRequest({
			tag: "AcpSetSessionConfigRequest",
			val: { sessionId, category, value },
		});
		if (response.tag !== "AcpSessionRpcResponse") {
			throw new Error(
				`unexpected response to AcpSetSessionConfigRequest: ${response.tag}`,
			);
		}
		return toJsonRpcResponse(JSON.parse(response.val.response));
	}

	private _removeSession(sessionId: string): void {
		this._sessions.delete(sessionId);
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
		this._rejectPendingPermissionReplies(sessionId);
		this._removeSession(sessionId);
		const response = await this._sendAcpRequest({
			tag: "AcpCloseSessionRequest",
			val: { sessionId },
		});
		if (response.tag !== "AcpSessionClosedResponse") {
			throw new Error(
				`unexpected response to AcpCloseSessionRequest: ${response.tag}`,
			);
		}
	}

	private async _getSessionState(
		sessionId: string,
	): Promise<SidecarSessionState> {
		const response = await this._sendAcpRequest({
			tag: "AcpGetSessionStateRequest",
			val: { sessionId },
		});
		if (response.tag !== "AcpSessionStateResponse") {
			throw new Error(
				`unexpected response to AcpGetSessionStateRequest: ${response.tag}`,
			);
		}
		return sidecarSessionStateFromAcp(response.val);
	}

	async createSession(
		agentType: AgentType,
		options?: CreateSessionOptions,
	): Promise<{ sessionId: string }> {
		// The client is npm-agnostic: it sends only the agent NAME. The sidecar
		// resolves the name -> package -> entrypoint/env/launchArgs from the
		// projected `/opt/agentos/<name>/current/agentos-package.json` and spawns
		// (including the agent's static launch args and manifest env defaults).
		// System-prompt assembly/injection (launch args / OPENCODE_CONTEXTPATHS) is
		// owned by the sidecar; the host only forwards additionalInstructions /
		// skipOsInstructions plus the caller's env.
		const response = await this._sendAcpRequest({
			tag: "AcpCreateSessionRequest",
			val: {
				agentType: String(agentType),
				runtime: null,
				args: null,
				env: options?.env ? new Map(Object.entries(options.env)) : null,
				cwd: options?.cwd ?? null,
				mcpServers: options?.mcpServers
					? JSON.stringify(options.mcpServers)
					: null,
				protocolVersion: null,
				clientCapabilities: null,
				additionalInstructions: options?.additionalInstructions ?? null,
				skipOsInstructions: options?.skipOsInstructions === true ? true : null,
			},
		});
		if (response.tag !== "AcpSessionCreatedResponse") {
			throw new Error(`unexpected create_session response: ${response.tag}`);
		}
		const state = await this._getSessionState(response.val.sessionId);
		this._sessions.set(state.sessionId, sessionEntryFromState(state));

		return { sessionId: state.sessionId };
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
	 * The new live session is registered locally only for host callbacks/events;
	 * authoritative state remains in the sidecar.
	 *
	 * Resume depends on a durable root; on a non-durable (default in-memory) root
	 * there is no surviving store and the fallback tier always runs.
	 */
	async resumeSession(
		sessionId: string,
		agentType: AgentType,
		options?: ResumeSessionOptions,
	): Promise<ResumeSessionResult> {
		// The client is npm-agnostic: it sends only the agent NAME. The sidecar
		// resolves the name -> package -> entrypoint/env/launchArgs from the
		// projected manifest, exactly as createSession does.
		const response = await this._sendAcpRequest({
			tag: "AcpResumeSessionRequest",
			val: {
				sessionId,
				agentType: String(agentType),
				transcriptPath: options?.transcriptPath ?? null,
				cwd: options?.cwd ?? null,
				env: options?.env ? new Map(Object.entries(options.env)) : null,
			},
		});
		if (response.tag !== "AcpSessionResumedResponse") {
			throw new Error(`unexpected resume_session response: ${response.tag}`);
		}
		const { sessionId: liveSessionId, mode } = response.val;

		const state = await this._getSessionState(liveSessionId);
		this._sessions.set(state.sessionId, sessionEntryFromState(state));

		return { sessionId: state.sessionId, mode };
	}

	private _installSidecarRequestHandler(): void {
		const context: HostCallbackContext = {
			toolMap: buildToolMap(this._toolKits),
		};
		this._sidecarClient.setSidecarRequestHandler((request) => {
			switch (request.payload.type) {
				case "host_callback":
					return handleHostCallback(request, context);
				case "js_bridge_call":
					return handleJsBridgeCall(request.payload, {
						resolveTarget: (mountId) => {
							const hostMountResolver = (
								this.#kernel as unknown as {
									hostFilesystemForMount?: (
										mountId: string,
									) => VirtualFileSystem | undefined;
								}
							).hostFilesystemForMount;
							if (hostMountResolver) {
								const filesystem = hostMountResolver.call(
									this.#kernel,
									mountId,
								);
								return filesystem ? { filesystem, rootPath: "/" } : undefined;
							}
							return { filesystem: this.#kernel.vfs, rootPath: mountId };
						},
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
				if (callback.val.timeoutMs > BigInt(Number.MAX_SAFE_INTEGER)) {
					throw new Error("ACP permission callback timeout exceeds JS range");
				}
				const reply = await this._handleAcpPermissionCallback(
					callback.val.sessionId,
					callback.val.permissionId,
					{
						...toRecord(JSON.parse(callback.val.params)),
						_acpMethod: ACP_PERMISSION_METHOD,
					},
					Number(callback.val.timeoutMs),
				);
				return {
					type: "ext_result",
					envelope: {
						namespace: ACP_EXTENSION_NAMESPACE,
						payload: encodeAcpCallbackResponse({
							tag: "AcpPermissionCallbackResponse",
							val: {
								permissionId: callback.val.permissionId,
								reply: reply ?? null,
							},
						}),
					},
				};
			}
			case "AcpHostRequestCallback":
				return {
					type: "ext_result",
					envelope: {
						namespace: ACP_EXTENSION_NAMESPACE,
						payload: encodeAcpCallbackResponse({
							tag: "AcpHostRequestCallbackResponse",
							val: { response: null },
						}),
					},
				};
		}
	}

	private async _handleAcpPermissionCallback(
		sessionId: string,
		permissionId: string,
		params: Record<string, unknown>,
		timeoutMs: number,
	): Promise<PermissionReply | undefined> {
		const session = this._sessions.get(sessionId);
		if (!session) {
			return undefined;
		}

		if (session.permissionHandlers.size === 0) {
			// Surface the absent host route, then let the sidecar apply its ACP
			// permission default. The client does not manufacture a policy answer.
			this._warnNoPermissionHandlerOnce(session, params);
			return undefined;
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
				}, timeoutMs);
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
		} catch (error) {
			console.warn(
				`ACP permission callback failed for ${sessionId}/${permissionId}; deferring to sidecar default`,
				error,
			);
			return undefined;
		}
	}

	/**
	 * Gracefully destroy a session: cancel any pending work, close the client,
	 * and remove from tracking. Unlike close() which is abrupt, this attempts
	 * a graceful shutdown sequence.
	 */
	async destroySession(sessionId: string): Promise<void> {
		await this._closeSessionInternal(sessionId);
	}

	// ── Flat session API (ID-based) ───────────────────────────────

	async prompt(sessionId: string, text: string): Promise<PromptResult> {
		const result = await this._sendSessionRequestWithText(
			sessionId,
			"session/prompt",
			{
				prompt: [{ type: "text", text }],
			},
		);
		if (result.text === null) {
			throw new Error("sidecar prompt response is missing accumulated text");
		}
		return { response: result.response, text: result.text };
	}

	/** Cancel ongoing agent work for a session. */
	async cancelSession(sessionId: string): Promise<JsonRpcResponse> {
		return this._sendSessionRequest(sessionId, "session/cancel");
	}

	async closeSession(sessionId: string): Promise<void> {
		await this._closeSessionInternal(sessionId);
	}

	async respondPermission(
		sessionId: string,
		permissionId: string,
		reply: PermissionReply,
	): Promise<JsonRpcResponse> {
		const session = this._sessions.get(sessionId);
		const pendingReply = session?.pendingPermissionReplies.get(permissionId);
		if (pendingReply) {
			session?.pendingPermissionReplies.delete(permissionId);
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

	async getSessionModes(sessionId: string): Promise<SessionModeState | null> {
		return toSessionModes((await this._getSessionState(sessionId)).modes);
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

	async getSessionConfigOptions(
		sessionId: string,
	): Promise<SessionConfigOption[]> {
		return toSessionConfigOptions(
			(await this._getSessionState(sessionId)).configOptions,
		);
	}

	async getSessionCapabilities(
		sessionId: string,
	): Promise<AgentCapabilities | null> {
		const caps = toAgentCapabilities(
			(await this._getSessionState(sessionId)).agentCapabilities,
		);
		return Object.keys(caps).length > 0 ? caps : null;
	}

	async getSessionAgentInfo(sessionId: string): Promise<AgentInfo | null> {
		return toAgentInfo((await this._getSessionState(sessionId)).agentInfo);
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
	async scheduleCron(options: CronJobOptions): Promise<CronJob> {
		return this._cronManager.schedule(options);
	}

	/** List all registered cron jobs. */
	async listCronJobs(): Promise<CronJobInfo[]> {
		return this._cronManager.list();
	}

	/** Cancel a cron job by ID. */
	async cancelCronJob(id: string): Promise<void> {
		await this._cronManager.cancel(id);
	}

	/** Subscribe to cron lifecycle events (fire, complete, error). */
	onCronEvent(handler: CronEventHandler): void {
		this._cronManager.onEvent(handler);
	}

	async dispose(): Promise<void> {
		this._cronManager.dispose();

		for (const sessionId of [...this._sessions.keys()]) {
			try {
				await this._closeSessionInternal(sessionId);
			} catch (error) {
				console.warn(
					`failed to close ACP session ${sessionId} during dispose`,
					error,
				);
			}
		}

		const shellKillResults = await Promise.allSettled(
			[...this._shells.values()].map((entry) => entry.handle.kill()),
		);
		for (const result of shellKillResults) {
			if (result.status === "rejected") {
				console.warn("failed to close shell during dispose", result.reason);
			}
		}
		const shellExitPromises = [...this._pendingShellExitPromises.values()];
		this._shells.clear();
		this._processes.clear();
		await waitForTrackedExitPromises(
			shellExitPromises,
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
	/**
	 * The shared sidecar's child process + stdio, cached for synchronous
	 * ref/unref. Unref'd when no VM leases are active so a one-shot host process
	 * can exit after `dispose()`; re-ref'd while leases are live.
	 */
	sharedChild?: SidecarEventLoopHandle;
	/**
	 * Number of live "holds" on the shared sidecar's event-loop reference. A hold
	 * is taken for the WHOLE create→use→dispose lifetime of every VM lease (not
	 * just while it sits in `activeLeases`), so a VM that is still mid-creation
	 * still counts. The child + stdio are ref'd while this is >0 and unref'd at 0.
	 * A counter (not a boolean) so concurrent create/dispose cannot clobber each
	 * other — Node ref/unref is not itself counted.
	 */
	eventLoopHolds?: number;
}

const sidecarStates = new WeakMap<AgentOsSidecar, AgentOsSidecarState>();
const sharedSidecars = new Map<string, AgentOsSidecar>();

interface RefCountableHandle {
	ref?(): unknown;
	unref?(): unknown;
}

interface SidecarEventLoopHandle extends RefCountableHandle {
	stdin?: RefCountableHandle | null;
	stdout?: RefCountableHandle | null;
	stderr?: RefCountableHandle | null;
	kill?(signal?: string | number): unknown;
}

let sidecarProcessExitHookInstalled = false;

/**
 * Install a one-time, synchronous `process.on("exit")` hook that SIGKILLs any
 * pooled shared sidecar child. Once a one-shot host process is allowed to exit
 * (its sidecar handles are unref'd at 0 leases), this reaps the sidecar
 * immediately instead of waiting for its stdin-EOF grace window — no orphan, no
 * delay. We deliberately do NOT install SIGINT/SIGTERM handlers: a library
 * should not hijack the host's signal handling. SIGINT still reaches the sidecar
 * via the process group, and SIGTERM-driven exit still closes its stdin.
 */
function ensureSidecarProcessExitCleanup(): void {
	if (sidecarProcessExitHookInstalled) return;
	sidecarProcessExitHookInstalled = true;
	process.on("exit", () => {
		for (const sidecar of sharedSidecars.values()) {
			try {
				sidecarStates.get(sidecar)?.sharedChild?.kill?.("SIGKILL");
			} catch {
				// best-effort reap; the process is exiting regardless
			}
		}
	});
}

function sidecarChildHandle(
	client: unknown,
): SidecarEventLoopHandle | undefined {
	// SidecarProcess -> StdioSidecarProtocolClient.child (the spawned ChildProcess).
	const protocolClient = (
		client as
			| { protocolClient?: { child?: SidecarEventLoopHandle } }
			| undefined
	)?.protocolClient;
	return protocolClient?.child ?? undefined;
}

/**
 * Apply the current hold state to the shared sidecar's child + stdio: ref them
 * while ≥1 hold is live so in-flight VM work keeps the host process alive; unref
 * them at 0 so a one-shot script exits on its own after `dispose()`. The sidecar
 * process itself stays running (reusable) and self-exits on stdin EOF when the
 * host finally goes away. Best-effort: never let ref/unref break VM lifecycle.
 */
function applySharedSidecarHold(state: AgentOsSidecarState): void {
	const child = state.sharedChild;
	if (!child) return;
	const hold = (state.eventLoopHolds ?? 0) > 0;
	for (const handle of [child, child.stdin, child.stdout, child.stderr]) {
		if (!handle) continue;
		try {
			if (hold) handle.ref?.();
			else handle.unref?.();
		} catch {
			// ref/unref is an optimization, not correctness-critical
		}
	}
}

/**
 * Take a hold for the entire create→use→dispose lifetime of one VM lease. Taken
 * BEFORE VM creation starts (not when the lease lands in `activeLeases`) so a VM
 * that is still mid-creation keeps the sidecar ref'd and a concurrent dispose
 * cannot unref it out from under the in-flight create.
 */
function acquireSharedSidecarHold(state: AgentOsSidecarState): void {
	state.eventLoopHolds = (state.eventLoopHolds ?? 0) + 1;
	if (state.eventLoopHolds === 1) applySharedSidecarHold(state);
}

/** Release a hold taken by {@link acquireSharedSidecarHold}; unref at 0. */
function releaseSharedSidecarHold(state: AgentOsSidecarState): void {
	const current = state.eventLoopHolds ?? 0;
	if (current <= 0) {
		// The `holdReleased` guard makes each lease release exactly once, so this
		// should be unreachable. Warn rather than silently floor, per the repo's
		// no-silent-masking rule, so an accounting bug surfaces instead of hiding.
		state.eventLoopHolds = 0;
		console.warn(
			"[agentos] shared sidecar event-loop hold released more than acquired",
		);
		return;
	}
	state.eventLoopHolds = current - 1;
	if (state.eventLoopHolds === 0) applySharedSidecarHold(state);
}

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
		ensureSidecarProcessExitCleanup();
		state.nativeProcess = (async () => {
			const client = SidecarProcess.spawn({
				command: resolvePublishedSidecarBinary(),
				args: [],
			});
			// Track the child immediately — BEFORE the handshake await — so a
			// failed `authenticateAndOpenSession()` can still reap it (otherwise
			// the spawned child is untracked, unreapable, and pins the loop).
			state.sharedChild = sidecarChildHandle(client);
			if (!state.sharedChild) {
				// We reached into @rivet-dev/agentos-runtime-core internals to get the child for
				// idle-unref. If that shape ever changes this returns undefined and
				// the optimization silently stops working (one-shot scripts would
				// hang again). Make it loud rather than a silent regression.
				console.warn(
					"[agentos] could not resolve the shared sidecar child handle; " +
						"standalone scripts may not exit cleanly after dispose(). " +
						"This usually means @rivet-dev/agentos-runtime-core internals changed.",
				);
			}
			// Apply the current hold state to the just-spawned child.
			applySharedSidecarHold(state);
			try {
				const session = await client.authenticateAndOpenSession();
				return { client, session };
			} catch (error) {
				// Spawn/handshake failed: reap the child, drop the cached handle,
				// and CLEAR the rejected promise so the next create() retries
				// instead of permanently wedging on a rejected `nativeProcess`.
				try {
					state.sharedChild?.kill?.("SIGKILL");
				} catch {
					// already gone
				}
				state.sharedChild = undefined;
				state.nativeProcess = undefined;
				throw error;
			}
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
	// The cached child is now dead; drop it (symmetric with the assignment in
	// ensureSharedSidecarNativeProcess). We deliberately do NOT zero
	// `eventLoopHolds` here: this runs only from `AgentOsSidecar.dispose()`, which
	// has already set the handle to `disposing` (so no new lease can acquire) and
	// drained `activeLeases`; the disposed handle's state is then abandoned. Force-
	// zeroing a shared counter could clobber a hold on a freshly re-acquired
	// process generation, so it is left to the balanced acquire/release pairs.
	state.sharedChild = undefined;
	try {
		const { client } = await pending;
		await client.dispose();
	} catch (error) {
		console.warn("failed to dispose shared sidecar process", error);
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

	// Hold the shared sidecar's event-loop ref for this lease's WHOLE lifetime —
	// taken now, before VM creation, so a concurrent dispose cannot unref the
	// sidecar while this create is still in flight. Released exactly once on
	// dispose or on a failed create.
	acquireSharedSidecarHold(state);
	let holdReleased = false;
	const releaseHold = () => {
		if (holdReleased) return;
		holdReleased = true;
		releaseSharedSidecarHold(state);
	};

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
				// Release this lease's hold; the shared sidecar is unref'd only
				// once the last hold (across all in-flight + active leases) drops,
				// so a one-shot host process can then exit on its own.
				releaseHold();
			},
		};

		leaseRecord = {
			dispose: () => lease.dispose(),
		};
		state.activeLeases.add(leaseRecord);
		state.description.activeVmCount = state.activeLeases.size;
		return lease;
	} catch (error) {
		await client.dispose().catch((cleanupError) => {
			console.warn(
				"failed to dispose sidecar client after lease creation failure",
				cleanupError,
			);
		});
		releaseHold();
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
