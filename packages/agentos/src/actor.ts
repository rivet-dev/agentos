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

import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import common from "@agentos-software/common";
import { OPT_AGENTOS_ROOT } from "@rivet-dev/agentos-core";
import { getSidecarPath } from "@rivet-dev/agentos-sidecar";
import {
	type ActorDefinition,
	type ActorFactoryHandle,
	actor,
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
import type { AgentOsActions } from "./generated/actor-actions.generated.js";
import { getPluginPath } from "./plugin-binary.js";
import type { AgentOsActorState, AgentOsActorVars } from "./types.js";

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

interface NormalizedPackageRef {
	path: string;
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
	// Single package reference: `packagePath` (packed `.aospkg` file, or a
	// transition package dir); a raw string is shorthand for the same path.
	if (typeof value === "string") {
		return { path: value };
	}
	const record = toRecord(value);
	if (typeof record.packagePath === "string") {
		return { path: record.packagePath };
	}
	// Recognizably-legacy shapes fail loudly instead of silently dropping the
	// package from the VM.
	for (const legacy of ["packageTar", "packageDir", "commandDir", "dir"]) {
		if (typeof record[legacy] === "string") {
			throw new Error(
				`agentOS package ref uses removed field "${legacy}"; packages are referenced ` +
					"by a single `packagePath` — rebuild @agentos-software/* dependencies or pass { packagePath }",
			);
		}
	}
	return undefined;
}

function normalizedPackageRefs(software: unknown[]): NormalizedPackageRef[] {
	const refs: NormalizedPackageRef[] = [];
	const seen = new Set<string>();
	for (const entry of software.flat()) {
		const ref = normalizePackageRef(entry);
		if (!ref || seen.has(ref.path)) continue;
		seen.add(ref.path);
		refs.push(ref);
	}
	return refs;
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
	const packages = packageRefs.map((ref) => ({ packagePath: ref.path }));
	const mounts = serializeNativeMounts(options.mounts);
	const sidecar = serializeSidecar(options.sidecar);
	return JSON.stringify({
		// The actor forwards ONLY package dirs; the sidecar resolves each agent from
		// the projected `/opt/agentos/<name>/current/agentos-package.json` (no
		// client-side adapter-entrypoint resolution — see root CLAUDE.md).
		packages,
		packagesMountAt: OPT_AGENTOS_ROOT,
		additionalInstructions: options.additionalInstructions,
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
		const record = mount as Record<string, unknown>;
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
	actorOptions: Record<string, unknown>,
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
			// RivetKit's native-plugin bridge owns the runtime ActorConfig for
			// cdylib actors, so forward the merged per-actor lifecycle options too.
			actorOptions,
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
			actorOptions?: Record<string, unknown>;
			inspectorTabs: typeof AGENTOS_INSPECTOR_CONFIG.tabs;
		};
		return runtime.createNativePluginFactory(options);
	};
}

/**
 * Type alias for the `agentOS(...)` return type. Events are not typed at the TS
 * surface because the Rust plugin owns the broadcast set, but the ACTIONS are
 * typed via {@link AgentOsActions} — generated from the Rust dispatch contract
 * table in `crates/agentos-actor-plugin/src/actions/contract_surface.rs`.
 * That is what gives `createClient<typeof registry>()` a fully-typed handle
 * (e.g. `handle.exec()` returns `ExecResult`, not `unknown`).
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
		{
			id: "transcript",
			label: "Transcript",
			source: INSPECTOR_TABS_ASSET_DIR,
			icon: "comments",
		},
		{
			id: "terminal",
			label: "Terminal",
			source: INSPECTOR_TABS_ASSET_DIR,
			icon: "terminal",
		},
		{
			id: "filesystem",
			label: "Filesystem",
			source: INSPECTOR_TABS_ASSET_DIR,
			icon: "folder-tree",
		},
		{
			id: "system",
			label: "System",
			source: INSPECTOR_TABS_ASSET_DIR,
			icon: "layer-group",
		},
		// Processes/software/mounts live as sections inside the System tab.
		// `metadata` is dashboard-owned and not hideable (the schema only
		// accepts the six built-in ids below), so it stays.
		...["workflow", "database", "state", "queue", "connections", "console"].map(
			(id) => ({ id, hidden: true as const }),
		),
	],
};

export function createAgentOS<TConnParams = undefined>(
	config: AgentOsActorConfigInput<TConnParams>,
): AgentOsActorDefinition<TConnParams> {
	const parsed = agentOsActorConfigSchema.parse(
		config,
	) as AgentOsActorConfig<TConnParams>;

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
	definition.nativeFactoryBuilder = buildNativeFactoryBuilder(
		parsed,
		actorOptions,
	);
	return definition;
}
