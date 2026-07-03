/**
 * Rust-backed `agentOS(...)` definition.
 *
 * Produces an `ActorDefinition` whose `nativeFactoryBuilder` constructs a
 * native-actor-plugin factory through `runtime.createNativePluginFactory(...)`
 * (NAPI → `dlopen` of the agent-os actor plugin cdylib, the inverse of the
 * generic host loader). All lifecycle, state, and action dispatch live in the
 * Rust plugin (`crates/agentos-actor-plugin`). This JS shim only validates
 * configuration, resolves the plugin + sidecar binaries, and hands the opaque
 * config envelope across the bridge — it owns no agent-os runtime logic.
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import common from "@agentos-software/common";
import {
	AgentOs,
	type AgentOsOptions,
	OPT_AGENTOS_BIN,
	OPT_AGENTOS_ROOT,
	parseAgentOsOptions,
} from "@rivet-dev/agentos-core";
import { getSidecarPath } from "@rivet-dev/agentos-sidecar";
import {
	actor,
	event,
	type ActorDefinition,
	type ActorFactoryHandle,
	type CoreRuntime,
	type DatabaseProvider,
	type NapiNativePluginOptions,
	type RawAccess,
} from "rivetkit";
import {
	type AgentOsActorConfig,
	type AgentOsActorConfigInput,
	agentOsActorConfigSchema,
	nativeAgentOsOptionsSchema,
} from "./config.js";
import { getPluginPath } from "./plugin-binary.js";
import type {
	AgentOsActions,
	DirEntry,
	MountInfo,
	SoftwareInfo,
	VmFetchOptions,
} from "./actor-actions.js";
import type {
	AgentOsActorState,
	AgentOsActorVars,
	AgentOsEvents,
} from "./types.js";

/**
 * Build the JSON envelope the Rust plugin consumes. The Rust deserializer
 * uses `deny_unknown_fields`, so the envelope must stay in lock-step with
 * `crates/agentos-actor-plugin/src/config.rs::AgentOsConfigJson`.
 *
 * Software threading: each software ref is flattened (meta packages such as
 * `common` are arrays of refs), normalized to a package dir, and forwarded as
 * `{ dir }` so the sidecar owns the `/opt/agentos` projection. Agent configs
 * are derived from each package's `agentos-package.json`, mirroring
 * `packages/core/src/agent-os.ts`.
 */
interface NativeMountLike {
	path: string;
	plugin: {
		id: string;
		config?: unknown;
	};
	readOnly?: boolean;
}

interface PackageAgentManifest {
	acpEntrypoint: string;
	env?: Record<string, string>;
	launchArgs?: string[];
	snapshot?: boolean;
}

interface PackageManifest {
	name: string;
	agent?: PackageAgentManifest;
}

interface NormalizedPackageRef {
	dir: string;
	legacyManifest?: PackageManifest;
}

interface SerializedAgentConfig {
	name: string;
	adapterEntrypoint: string;
	launchArgs?: string[];
	defaultEnv?: Record<string, string>;
}

/**
 * A native `host_dir` mount of a host `node_modules` directory at
 * `/root/node_modules`, the serializable form `agentOS({ options: { mounts } })`
 * accepts across the NAPI boundary.
 */
export interface NodeModulesMountConfig {
	path: "/root/node_modules";
	plugin: { id: "host_dir"; config: { hostPath: string; readOnly: boolean } };
	readOnly: boolean;
}

/**
 * Mount a host `node_modules` directory into the VM at `/root/node_modules`.
 *
 * This is the explicit, mount-based replacement for the removed `moduleAccessCwd`
 * mechanism: the VM module resolver reads the mounted tree through the kernel
 * VFS, so the caller supplies exactly the `node_modules` directory whose
 * packages should resolve in the guest.
 *
 * @param hostNodeModulesDir Absolute host path to a `node_modules` directory.
 * @param opts.readOnly Defaults to `true`; the mount is read-only.
 */
export function nodeModulesMount(
	hostNodeModulesDir: string,
	opts?: { readOnly?: boolean },
): NodeModulesMountConfig {
	const readOnly = opts?.readOnly ?? true;
	return {
		path: "/root/node_modules",
		plugin: {
			id: "host_dir",
			config: { hostPath: hostNodeModulesDir, readOnly },
		},
		readOnly,
	};
}

function toRecord(value: unknown): Record<string, unknown> {
	return value && typeof value === "object" && !Array.isArray(value)
		? (value as Record<string, unknown>)
		: {};
}

function normalizePackageRef(value: unknown): NormalizedPackageRef | undefined {
	if (typeof value === "string") {
		return { dir: value };
	}
	const record = toRecord(value);
	if (typeof record.packageDir === "string") {
		return {
			dir: record.packageDir,
			legacyManifest: legacyPackageManifest(record),
		};
	}
	if (typeof record.dir === "string") {
		return {
			dir: record.dir,
			legacyManifest: legacyPackageManifest(record),
		};
	}
	return undefined;
}

function legacyPackageManifest(
	record: Record<string, unknown>,
): PackageManifest | undefined {
	if (typeof record.name !== "string") {
		return undefined;
	}
	const manifest: PackageManifest = { name: record.name };
	const agent = toRecord(record.agent);
	if (typeof agent.acpEntrypoint === "string") {
		manifest.agent = {
			acpEntrypoint: agent.acpEntrypoint,
			...(isStringRecord(agent.env) ? { env: agent.env } : {}),
			...(Array.isArray(agent.launchArgs) &&
			agent.launchArgs.every((arg) => typeof arg === "string")
				? { launchArgs: agent.launchArgs }
				: {}),
			...(typeof agent.snapshot === "boolean" ? { snapshot: agent.snapshot } : {}),
		};
	}
	return manifest;
}

function readPackageManifestForClient(
	ref: NormalizedPackageRef,
): PackageManifest | undefined {
	return tryReadAgentosPackageManifest(ref.dir) ?? ref.legacyManifest;
}

function tryReadAgentosPackageManifest(
	dir: string,
): PackageManifest | undefined {
	try {
		return readAgentosPackageManifest(dir);
	} catch (error) {
		if (error instanceof Error && errorCode(error) === "ENOENT") {
			return undefined;
		}
		throw error;
	}
}

function readAgentosPackageManifest(dir: string): PackageManifest {
	const manifestPath = join(dir, "agentos-package.json");
	let parsed: unknown;
	try {
		parsed = JSON.parse(readFileSync(manifestPath, "utf8"));
	} catch (error) {
		const wrapped = new Error(
			`Failed to read agentOS package manifest at ${manifestPath}: ${error instanceof Error ? error.message : String(error)}`,
		);
		const code = errorCode(error);
		if (code !== undefined) {
			Object.assign(wrapped, { code });
		}
		throw wrapped;
	}
	return validateAgentosPackageManifest(parsed, manifestPath);
}

function errorCode(error: unknown): string | undefined {
	if (!isPlainObject(error)) {
		return undefined;
	}
	return typeof error.code === "string" ? error.code : undefined;
}

function validateAgentosPackageManifest(
	value: unknown,
	source: string,
): PackageManifest {
	if (!isPlainObject(value) || typeof value.name !== "string") {
		throw new Error(`Invalid agentOS package manifest at ${source}: missing name`);
	}
	const manifest: PackageManifest = { name: value.name };
	if (value.agent !== undefined) {
		if (
			!isPlainObject(value.agent) ||
			typeof value.agent.acpEntrypoint !== "string"
		) {
			throw new Error(
				`Invalid agentOS package manifest at ${source}: invalid agent.acpEntrypoint`,
			);
		}
		manifest.agent = {
			acpEntrypoint: value.agent.acpEntrypoint,
			...(isStringRecord(value.agent.env) ? { env: value.agent.env } : {}),
			...(Array.isArray(value.agent.launchArgs) &&
			value.agent.launchArgs.every((arg) => typeof arg === "string")
				? { launchArgs: value.agent.launchArgs }
				: {}),
			...(typeof value.agent.snapshot === "boolean"
				? { snapshot: value.agent.snapshot }
				: {}),
		};
	}
	return manifest;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringRecord(value: unknown): value is Record<string, string> {
	return (
		value !== null &&
		typeof value === "object" &&
		!Array.isArray(value) &&
		Object.values(value).every((entry) => typeof entry === "string")
	);
}

function normalizedPackageRefs(software: unknown[]): NormalizedPackageRef[] {
	const refs: NormalizedPackageRef[] = [];
	const seen = new Set<string>();
	for (const entry of software.flat()) {
		const ref = normalizePackageRef(entry);
		if (!ref || seen.has(ref.dir)) continue;
		seen.add(ref.dir);
		refs.push(ref);
	}
	return refs;
}

function serializedAgentConfigs(
	packageRefs: NormalizedPackageRef[],
): SerializedAgentConfig[] {
	const configs: SerializedAgentConfig[] = [];
	for (const ref of packageRefs) {
		const manifest = readPackageManifestForClient(ref);
		if (!manifest?.agent) continue;
		configs.push({
			name: manifest.name,
			adapterEntrypoint: `${OPT_AGENTOS_BIN}/${manifest.agent.acpEntrypoint}`,
			launchArgs: manifest.agent.launchArgs,
			defaultEnv: manifest.agent.env,
		});
	}
	return configs;
}

export function buildConfigJson<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
): string {
	const options = nativeAgentOsOptionsSchema.parse(
		parsed.options ?? {},
	) as Record<string, unknown>;
	const softwareInput = Array.isArray(options.software) ? options.software : [];
	const defaultSoftwareEnabled = options.defaultSoftware !== false;
	const packageRefs = normalizedPackageRefs(
		defaultSoftwareEnabled ? [common, ...softwareInput] : softwareInput,
	);
	const packages = packageRefs.map((ref) => ({ dir: ref.dir }));
	const agentConfigs = serializedAgentConfigs(packageRefs);
	const mounts = serializeNativeMounts(options.mounts);
	const sidecar = serializeSidecar(options.sidecar);
	return JSON.stringify({
		packages,
		packagesMountAt: OPT_AGENTOS_ROOT,
		agentConfigs,
		additionalInstructions: options.additionalInstructions,
		moduleAccessCwd: options.moduleAccessCwd,
		loopbackExemptPorts: options.loopbackExemptPorts,
		allowedNodeBuiltins: options.allowedNodeBuiltins,
		permissions: options.permissions,
		rootFilesystem: options.rootFilesystem,
		mounts,
		limits: options.limits,
		sidecar,
	});
}

function serializeNativeMounts(input: unknown): NativeMountLike[] | undefined {
	if (input == null) return undefined;
	if (!Array.isArray(input)) {
		throw new Error("agentOS() options.mounts must be an array");
	}
	return input.map((mount, index) => {
		if (!mount || typeof mount !== "object") {
			throw new Error(`agentOS() options.mounts[${index}] must be an object`);
		}
		const record = mount as unknown as Record<string, unknown>;
		if (record.driver !== undefined) {
			throw new Error(
				"agentOS() only supports Native mounts across the NAPI boundary; Plain mounts with driver callbacks are not serializable",
			);
		}
		if (record.filesystem !== undefined) {
			throw new Error(
				"agentOS() only supports Native mounts across the NAPI boundary; Overlay mounts are not serializable",
			);
		}
		const plugin = record.plugin;
		if (
			typeof record.path !== "string" ||
			!plugin ||
			typeof plugin !== "object" ||
			typeof (plugin as Record<string, unknown>).id !== "string"
		) {
			throw new Error(
				`agentOS() options.mounts[${index}] must be a Native mount with { path, plugin: { id, config? } }`,
			);
		}
		return {
			path: record.path,
			plugin: {
				id: (plugin as Record<string, unknown>).id as string,
				config: (plugin as Record<string, unknown>).config,
			},
			readOnly:
				typeof record.readOnly === "boolean" ? record.readOnly : undefined,
		};
	});
}

function serializeSidecar(input: unknown): { pool?: string } | undefined {
	if (input == null) return undefined;
	if (!input || typeof input !== "object") {
		throw new Error("agentOS() options.sidecar must be an object");
	}
	const record = input as Record<string, unknown>;
	if (record.kind === "explicit" || record.handle !== undefined) {
		throw new Error(
			"agentOS() only supports sidecar shared pool configuration across the NAPI boundary; explicit sidecar handles are not serializable",
		);
	}
	if (record.kind !== undefined && record.kind !== "shared") {
		throw new Error('agentOS() options.sidecar.kind must be "shared"');
	}
	return typeof record.pool === "string" ? { pool: record.pool } : {};
}

function buildNativeFactoryBuilder<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
): (runtime: CoreRuntime) => ActorFactoryHandle {
	return (runtime) => {
		if (runtime.kind !== "napi") {
			throw new Error(
				`agentOS() is only supported on the native NAPI runtime (current runtime kind: ${runtime.kind})`,
			);
		}
		if (!runtime.createNativePluginFactory) {
			throw new Error(
				"runtime.createNativePluginFactory is not implemented on the active CoreRuntime",
			);
		}
		const options: NapiNativePluginOptions = {
			// Resolve the prebuilt agent-os actor plugin cdylib; RivetKit `dlopen`s
			// it through the generic native-plugin ABI.
			pluginPath: getPluginPath(),
			// Opaque config envelope the plugin parses (config.rs::AgentOsConfigJson).
			configJson: buildConfigJson(parsed),
			// Resolve the prebuilt sidecar binary from the npm package so the plugin
			// spawns the bundled binary rather than relying on `agentos-sidecar`
			// being on PATH.
			sidecarPath: getSidecarPath(),
			// Custom inspector tabs. The native-plugin path bypasses the normal
			// actor-config assembly (`buildActorConfig`/`inspectorTabs`), so the
			// tabs MUST ride on the plugin options: the Rust NAPI binding
			// `from_native_plugin` forwards `inspectorTabs` into the actor config so
			// the dashboard serves `/inspector/custom-tabs/<id>/` and advertises them
			// in `tab-config`. (Setting `actor({ inspector })` alone does nothing for
			// native-plugin actors.)
			inspectorTabs: AGENTOS_INSPECTOR_CONFIG.tabs,
		} as NapiNativePluginOptions & {
			inspectorTabs: typeof AGENTOS_INSPECTOR_CONFIG.tabs;
		};
		return runtime.createNativePluginFactory(options);
	};
}

/**
 * Type alias for the `agentOS(...)` return type. Events are not typed at the TS
 * surface because the Rust plugin owns the broadcast set, but the ACTIONS are
 * typed via {@link AgentOsActions} — a TS mirror of the Rust dispatch in
 * `crates/agentos-actor-plugin/src/actions/mod.rs`. That is what gives
 * `createClient<typeof registry>()` a fully-typed handle (e.g. `handle.exec()`
 * returns `ExecResult`, not `unknown`). Keep the two in sync.
 */
export type AgentOsActorDefinition<TConnParams> = ActorDefinition<
	AgentOsActorState,
	TConnParams,
	undefined,
	AgentOsActorVars,
	undefined,
	DatabaseProvider<RawAccess>,
	Record<never, never>,
	Record<never, never>,
	AgentOsActions
>;

// One hour — far past any normal agent turn, connection setup, or idle gap, but
// still a finite bound (never `0`/Infinity) per the limits-and-observability
// policy. Agent turns routinely run minutes; the stock RivetKit defaults
// (actionTimeout 60s, on{Before,}ConnectTimeout 5s, sleepTimeout 30s) cut them
// off mid-flight and broke live `sessionEvent` streaming with
// "actor websocket connection setup timed out after 5000 ms".
const ACTOR_NEVER_HIT_MS = 60 * 60 * 1000;
// 512 MiB — large prompts/results stream as single actor messages; the stock
// 64 KiB incoming / 1 MiB outgoing caps truncate real agent payloads.
const ACTOR_NEVER_HIT_MESSAGE_BYTES = 512 * 1024 * 1024;

/**
 * Never-hit-by-normal-use defaults for the AgentOS actor. Every value is a high
 * but finite bound so a long multi-step agent turn, a slow connection setup, a
 * large prompt/result, and live `sessionEvent` streaming all complete without
 * tripping a RivetKit actor default. Callers can still override any single knob
 * via `actorOptions` (their value wins over these defaults).
 */
export const DEFAULT_AGENTOS_ACTOR_OPTIONS = {
	// Connection/setup lifecycle (stock 5s each) — the websocket setup path that
	// was timing out at 5000ms and dropping all streamed events.
	onBeforeConnectTimeout: ACTOR_NEVER_HIT_MS,
	onConnectTimeout: ACTOR_NEVER_HIT_MS,
	createVarsTimeout: ACTOR_NEVER_HIT_MS,
	createConnStateTimeout: ACTOR_NEVER_HIT_MS,
	onMigrateTimeout: ACTOR_NEVER_HIT_MS,
	// Action/RPC lifecycle (stock 60s) — long multi-step prompt turns.
	actionTimeout: ACTOR_NEVER_HIT_MS,
	// Idle/keepalive — don't reap a live session or sleep mid-turn (stock
	// connectionLivenessTimeout 2.5s, sleepTimeout 30s). The liveness *interval*
	// (ping cadence) is intentionally left at its small default.
	connectionLivenessTimeout: ACTOR_NEVER_HIT_MS,
	sleepTimeout: ACTOR_NEVER_HIT_MS,
	// Payload sizes — large prompts/results. `maxQueueMessageSize` is the
	// per-actor message cap (stock 64 KiB); the transport-level
	// max{Incoming,Outgoing}MessageSize live on the registry/setup config (see
	// AGENTOS_REGISTRY_MESSAGE_SIZE_DEFAULTS), not on per-actor options.
	maxQueueSize: 1_000_000,
	maxQueueMessageSize: ACTOR_NEVER_HIT_MESSAGE_BYTES,
} as const;

// Absolute path to the built inspector-tabs app (the shared Vite bundle). All
// custom tabs share this one `source` dir; the app routes on the
// `/inspector/custom-tabs/<id>/` URL segment. Resolves from both `src/` (tsx dev
// / the demo) and the published `dist/`, since `assets/` sits at the package
// root in both layouts.
const INSPECTOR_TABS_ASSET_DIR = join(
	dirname(fileURLToPath(import.meta.url)),
	"..",
	"assets",
	"inspector-tabs-app",
);

// Custom inspector tabs shipped by agent-os. Ids MUST match the `TABS` registry
// in `src/inspector-tabs/main.tsx`. The built-in rivetkit tabs are hidden so the
// dashboard shows only the agent-os tabs.
const AGENTOS_INSPECTOR_CONFIG = {
	tabs: [
		{ id: "transcript", label: "Transcript", source: INSPECTOR_TABS_ASSET_DIR, icon: "comments" },
		{ id: "filesystem", label: "Filesystem", source: INSPECTOR_TABS_ASSET_DIR, icon: "folder-tree" },
		{ id: "processes", label: "Processes", source: INSPECTOR_TABS_ASSET_DIR, icon: "microchip" },
		{ id: "software", label: "Software", source: INSPECTOR_TABS_ASSET_DIR, icon: "box-archive" },
		{ id: "mounts", label: "Mounts", source: INSPECTOR_TABS_ASSET_DIR, icon: "hard-drive" },
		...(["workflow", "database", "state", "queue", "connections", "console"].map(
			(id) => ({ id, hidden: true as const }),
		)),
	],
};

const AGENTOS_EVENTS = {
	sessionEvent: event<AgentOsEvents["sessionEvent"]>(),
	permissionRequest: event<AgentOsEvents["permissionRequest"]>(),
	vmBooted: event<AgentOsEvents["vmBooted"]>(),
	vmShutdown: event<AgentOsEvents["vmShutdown"]>(),
	processOutput: event<AgentOsEvents["processOutput"]>(),
	processExit: event<AgentOsEvents["processExit"]>(),
	shellData: event<AgentOsEvents["shellData"]>(),
	shellStderr: event<AgentOsEvents["shellStderr"]>(),
	shellExit: event<AgentOsEvents["shellExit"]>(),
	cronEvent: event<AgentOsEvents["cronEvent"]>(),
};

function requiresJsActor<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
): boolean {
	return Boolean(
		parsed.createOptions || parsed.options?.bindings || parsed.options?.sandbox,
	);
}

function hasSandboxClient(options: AgentOsOptions | undefined): boolean {
	return Boolean(
		options?.sandbox &&
			typeof options.sandbox === "object" &&
			"client" in options.sandbox,
	);
}

function splitCreateOptionsResult(
	result: unknown,
): { options: AgentOsOptions; dispose?: () => void | Promise<void> } {
	if (
		result &&
		typeof result === "object" &&
		!Array.isArray(result) &&
		"options" in result
	) {
		const record = result as {
			options?: AgentOsOptions;
			dispose?: () => void | Promise<void>;
		};
		return {
			options: record.options ?? {},
			dispose: record.dispose,
		};
	}
	return { options: (result ?? {}) as AgentOsOptions };
}

async function resolveActorVmOptions<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
	c: unknown,
): Promise<{ options: AgentOsOptions; dispose?: () => void | Promise<void> }> {
	const created = parsed.createOptions
		? splitCreateOptionsResult(await parsed.createOptions(c as never))
		: { options: {} };
	return {
		options: parseAgentOsOptions({
			...(parsed.options ?? {}),
			...created.options,
		}),
		dispose: created.dispose,
	};
}

function requireActorVm(c: { vars: AgentOsActorVars }): AgentOs {
	if (!c.vars.agentOs) {
		throw new Error("agentOS VM is not running");
	}
	return c.vars.agentOs;
}

function dirEntryType(stat: { isSymbolicLink?: boolean; isDirectory?: boolean }): DirEntry["type"] {
	if (stat.isSymbolicLink) return "symlink";
	if (stat.isDirectory) return "directory";
	return "file";
}

async function readdirEntries(vm: AgentOs, path: string): Promise<DirEntry[]> {
	const names = await vm.readdir(path);
	return Promise.all(
		names
			.filter((name) => name !== "." && name !== "..")
			.map(async (name) => {
				const childPath = path === "/" ? `/${name}` : `${path}/${name}`;
				const stat = await vm.stat(childPath);
				return {
					path: childPath,
					name,
					type: dirEntryType(stat),
				};
			}),
	);
}

function unsupportedDynamicAction(name: string): never {
	throw new Error(`agentOS createOptions actor does not support ${name} yet`);
}

async function disposeActorVm(
	c: { vars: AgentOsActorVars; broadcast: (name: string, payload: unknown) => void },
	reason: AgentOsEvents["vmShutdown"]["reason"],
): Promise<void> {
	const vm = c.vars.agentOs;
	const disposeCreateOptions = c.vars.disposeCreateOptions;
	c.vars.agentOs = null;
	c.vars.disposeCreateOptions = undefined;
	await Promise.allSettled([
		vm?.dispose(),
		disposeCreateOptions?.(),
		...c.vars.activeHooks,
	]);
	c.broadcast("vmShutdown", { reason });
}

function toMountInfo(options: AgentOsOptions): MountInfo[] {
	return (options.mounts ?? []).map((mount) => {
		const record = mount as unknown as Record<string, unknown>;
		const plugin = record.plugin as Record<string, unknown> | undefined;
		return {
			path: String(record.path ?? ""),
			kind:
				typeof plugin?.id === "string"
					? (plugin.id as MountInfo["kind"])
					: "host_dir",
			config: plugin?.config ?? null,
			readOnly: record.readOnly === true,
		};
	});
}

function toSoftwareInfo(options: AgentOsOptions): SoftwareInfo[] {
	const software = Array.isArray(options.software) ? options.software.flat() : [];
	return software.map((entry) => {
		const record =
			entry && typeof entry === "object"
				? (entry as unknown as Record<string, unknown>)
				: {};
		return {
			package:
				typeof record.name === "string"
					? record.name
					: typeof entry === "string"
						? entry
						: typeof record.packageDir === "string"
							? record.packageDir
							: typeof record.dir === "string"
								? record.dir
								: "unknown",
			kind: "tool",
			version: typeof record.version === "string" ? record.version : null,
		};
	});
}

const jsActorActions: AgentOsActions = {
	readFile: async (c, path) => requireActorVm(c).readFile(path),
	writeFile: async (c, path, content) => requireActorVm(c).writeFile(path, content),
	stat: async (c, path) => requireActorVm(c).stat(path),
	mkdir: async (c, path) => requireActorVm(c).mkdir(path, { recursive: true }),
	readdir: async (c, path) => readdirEntries(requireActorVm(c), path),
	exists: async (c, path) => requireActorVm(c).exists(path),
	move: async (c, from, to) => requireActorVm(c).move(from, to),
	deleteFile: async (c, path, options) => requireActorVm(c).delete(path, options),
	writeFiles: async (c, entries) =>
		(await requireActorVm(c).writeFiles(entries)).map((result) => ({
			path: result.path,
			ok: result.success,
			error: result.error,
		})),
	readFiles: async (c, paths) =>
		(await requireActorVm(c).readFiles(paths)).map((result) => ({
			path: result.path,
			...(result.content ? { content: result.content } : {}),
			error: result.error,
		})),
	readdirRecursive: async (c, path) =>
		(await requireActorVm(c).readdirRecursive(path)).map((entry) => ({
			path: entry.path,
			name: entry.path.split("/").filter(Boolean).at(-1) ?? entry.path,
			type: entry.type,
		})),

	exec: async (c, command) => requireActorVm(c).exec(command),
	spawn: async (c, command, args, options) => {
		const vm = requireActorVm(c);
		let pid = 0;
		const spawned = vm.spawn(command, args, {
			...options,
			onStdout: (data) => c.broadcast("processOutput", { pid, stream: "stdout", data }),
			onStderr: (data) => c.broadcast("processOutput", { pid, stream: "stderr", data }),
		});
		pid = spawned.pid;
		c.vars.activeProcesses.add(pid);
		const hook = vm.waitProcess(pid).then((exitCode) => {
			c.vars.activeProcesses.delete(pid);
			c.broadcast("processExit", { pid, exitCode });
		});
		c.vars.activeHooks.add(hook);
		void hook.finally(() => c.vars.activeHooks.delete(hook));
		return { pid };
	},
	waitProcess: async (c, pid) => requireActorVm(c).waitProcess(pid),
	killProcess: async (c, pid) => {
		requireActorVm(c).killProcess(pid);
	},
	stopProcess: async (c, pid) => {
		requireActorVm(c).stopProcess(pid);
	},
	listProcesses: async (c) => requireActorVm(c).listProcesses() as never,
	allProcesses: async (c) => requireActorVm(c).allProcesses(),
	processTree: async (c) => requireActorVm(c).processTree() as never,
	getProcess: async (c, pid) => requireActorVm(c).getProcess(pid) as never,
	writeProcessStdin: async (c, pid, data) => requireActorVm(c).writeProcessStdin(pid, data),
	closeProcessStdin: async (c, pid) => requireActorVm(c).closeProcessStdin(pid),

	openShell: async (c, options) => {
		const vm = requireActorVm(c);
		let shellId = "";
		const opened = vm.openShell({
			...options,
			onStderr: (data) => c.broadcast("shellStderr", { shellId, data }),
		});
		shellId = opened.shellId;
		c.vars.activeShells.add(shellId);
		vm.onShellData(shellId, (data) => c.broadcast("shellData", { shellId, data }));
		const hook = vm.waitShell(shellId).then((exitCode) => {
			c.vars.activeShells.delete(shellId);
			c.broadcast("shellExit", { shellId, exitCode });
		});
		c.vars.activeHooks.add(hook);
		void hook.finally(() => c.vars.activeHooks.delete(hook));
		return { shellId };
	},
	writeShell: async (c, shellId, data) => requireActorVm(c).writeShell(shellId, data),
	resizeShell: async (c, shellId, cols, rows) => {
		requireActorVm(c).resizeShell(shellId, cols, rows);
	},
	closeShell: async (c, shellId) => {
		requireActorVm(c).closeShell(shellId);
	},
	waitShell: async (c, shellId) => requireActorVm(c).waitShell(shellId),

	vmFetch: async (c, port, url, options?: VmFetchOptions) => {
		const response = await requireActorVm(c).fetch(
			port,
			new Request(url, options as RequestInit),
		);
		const headers: Record<string, string> = {};
		response.headers.forEach((value, key) => {
			headers[key] = value;
		});
		return {
			status: response.status,
			statusText: response.statusText,
			headers,
			body: new Uint8Array(await response.arrayBuffer()),
		};
	},

	scheduleCron: async (c, options) => {
		const action = options.action;
		const job = requireActorVm(c).scheduleCron({
			...options,
			action: {
				type: "callback",
				fn:
					action.type === "exec"
						? async () => {
								await requireActorVm(c).exec([
									action.command,
									...(action.args ?? []),
								].join(" "));
							}
						: async () => {
								const { sessionId } = await requireActorVm(c).createSession(
									action.agentType,
									{ cwd: action.cwd },
								);
								await requireActorVm(c).prompt(sessionId, action.prompt);
							},
			},
		});
		return { id: job.id };
	},
	listCronJobs: async (c) => requireActorVm(c).listCronJobs() as never,
	cancelCronJob: async (c, id) => {
		requireActorVm(c).cancelCronJob(id);
	},

	createSession: async (c, agentType, options) => {
		const vm = requireActorVm(c);
		const { sessionId } = await vm.createSession(agentType, options);
		c.vars.sessions.add(sessionId);
		c.vars.activeSessionIds.add(sessionId);
		vm.onSessionEvent(sessionId, (sessionEvent) => {
			c.broadcast("sessionEvent", { sessionId, event: sessionEvent });
		});
		vm.onPermissionRequest(sessionId, (request) => {
			c.broadcast("permissionRequest", { sessionId, request });
		});
		return sessionId;
	},
	sendPrompt: async (c, sessionId, text) => requireActorVm(c).prompt(sessionId, text),
	closeSession: async (c, sessionId) => {
		await requireActorVm(c).destroySession(sessionId);
		c.vars.activeSessionIds.delete(sessionId);
	},
	listPersistedSessions: async (c) =>
		requireActorVm(c).listSessions().map((session) => ({
			...session,
			capabilities: {},
			agentInfo: null,
			createdAt: Date.now(),
		})),
	getSessionEvents: async () => [],

	createSignedPreviewUrl: async () => unsupportedDynamicAction("createSignedPreviewUrl"),
	expireSignedPreviewUrl: async () => unsupportedDynamicAction("expireSignedPreviewUrl"),

	listMounts: async (c) => toMountInfo(c.vars.options),
	listSoftware: async (c) => toSoftwareInfo(c.vars.options),
};

function createJsAgentOS<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
	actorOptions: Record<string, unknown>,
): AgentOsActorDefinition<TConnParams> {
	return actor({
		events: AGENTOS_EVENTS,
		actions: jsActorActions,
		options: actorOptions,
		inspector: AGENTOS_INSPECTOR_CONFIG,
		onBeforeConnect: parsed.onBeforeConnect,
		createVars: async (c) => {
			const { options, dispose } = await resolveActorVmOptions(parsed, c);
			let vm: AgentOs | undefined;
			try {
				vm = await AgentOs.create(options);
				return {
					agentOs: vm,
					activeSessionIds: new Set<string>(),
					activeProcesses: new Set<number>(),
					activeHooks: new Set<Promise<void>>(),
					activeShells: new Set<string>(),
					sessions: new Set<string>(),
					options,
					disposeCreateOptions: dispose,
				};
			} catch (error) {
				await Promise.allSettled([vm?.dispose(), dispose?.()]);
				throw error;
			}
		},
		onWake: (c) => {
			c.broadcast("vmBooted", {});
		},
		onSleep: (c) => disposeActorVm(c as never, "sleep"),
		onDestroy: (c) => disposeActorVm(c as never, "destroy"),
	} as Parameters<typeof actor>[0]) as unknown as AgentOsActorDefinition<TConnParams>;
}

export function createAgentOS<TConnParams = undefined>(
	config: AgentOsActorConfigInput<TConnParams>,
): AgentOsActorDefinition<TConnParams> {
	const parsed = agentOsActorConfigSchema.parse(
		config,
	) as AgentOsActorConfig<TConnParams>;
	if (hasSandboxClient(parsed.options)) {
		throw new Error(
			"agentOS actor sandbox clients must be returned from createOptions so each actor instance gets its own sandbox client. Top-level sandbox: { provider } is allowed.",
		);
	}

	// Construct a minimal definition through the existing actor() helper, then
	// attach the Rust factory builder marker. The actions block stays empty
	// because no JS-side action ever runs: the engine driver branches on
	// `nativeFactoryBuilder` before reaching the JS dispatch path.
	const userActorOptions = (
		parsed as { actorOptions?: Record<string, unknown> }
	).actorOptions;
	// High never-hit defaults, with any caller-supplied option winning.
	const actorOptions = {
		...DEFAULT_AGENTOS_ACTOR_OPTIONS,
		...(userActorOptions ?? {}),
	};
	if (requiresJsActor(parsed)) {
		return createJsAgentOS(parsed, actorOptions);
	}
	nativeAgentOsOptionsSchema.parse(parsed.options ?? {});
	const definition = actor({
		actions: {},
		options: actorOptions,
		// Register the custom agent-os inspector tabs (and hide the built-in
		// rivetkit tabs) so the dashboard renders the agent-os UI. Without this
		// the shipped tab assets are never surfaced.
		inspector: AGENTOS_INSPECTOR_CONFIG,
	} as Parameters<
		typeof actor
	>[0]) as unknown as AgentOsActorDefinition<TConnParams>;
	definition.nativeFactoryBuilder = buildNativeFactoryBuilder(parsed);
	return definition;
}
