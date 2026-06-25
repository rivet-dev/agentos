/**
 * Rust-backed `agentOs(...)` definition.
 *
 * Produces an `ActorDefinition` whose `nativeFactoryBuilder` constructs a
 * native-actor-plugin factory through `runtime.createNativePluginFactory(...)`
 * (NAPI → `dlopen` of the agent-os actor plugin cdylib, the inverse of the
 * generic host loader). All lifecycle, state, and action dispatch live in the
 * Rust plugin (`crates/agentos-actor-plugin`). This JS shim only validates
 * configuration, resolves the plugin + sidecar binaries, and hands the opaque
 * config envelope across the bridge — it owns no agent-os runtime logic.
 */

import { sep } from "node:path";
import common from "@agentos-software/common";
import { getSidecarPath } from "@rivet-dev/agentos-sidecar";
import {
	actor,
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
import type { AgentOsActions } from "./actor-actions.js";
import type { AgentOsActorState, AgentOsActorVars } from "./types.js";

/**
 * Build the JSON envelope the Rust plugin consumes. The Rust deserializer
 * uses `deny_unknown_fields`, so the envelope must stay in lock-step with
 * `crates/agentos-actor-plugin/src/config.rs::AgentOsConfigJson`.
 *
 * Software threading: each software descriptor is flattened (meta packages
 * such as `common` are arrays of descriptors) and mapped to the Rust
 * `SoftwareInput { package, kind }`. The agentos-client resolves an
 * ABSOLUTE `package` directly (its `resolve_software` lets an absolute path
 * bypass the `node_modules` prefix), so the descriptor's already-resolved
 * `commandDir` (wasm commands) / `packageDir` (agents/tools) is forwarded as
 * `package`.
 */
interface SoftwareDescriptorLike {
	commandDir?: string;
	packageDir?: string;
	requires?: string[];
	agent?: unknown;
	hostTool?: unknown;
	toolkit?: unknown;
}

interface NativeMountLike {
	path: string;
	plugin: {
		id: string;
		config?: unknown;
	};
	readOnly?: boolean;
}

/**
 * A native `host_dir` mount of a host `node_modules` directory at
 * `/root/node_modules`, the serializable form `agentOs({ options: { mounts } })`
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

/**
 * Derive the `node_modules` root that contains an installed package directory.
 * For an agent descriptor whose `packageDir` is `<root>/node_modules/@scope/pkg`,
 * this returns `<root>/node_modules` — the hoist root that also holds the agent's
 * `requires` (the ACP adapter + agent SDK) and their transitive deps under a
 * flat (npm) install. Returns `undefined` when `packageDir` is not inside a
 * `node_modules` tree (e.g. a linked monorepo checkout), where the caller must
 * supply an explicit `nodeModulesMount(...)`.
 */
function nodeModulesRootOf(packageDir: string): string | undefined {
	const parts = packageDir.split(sep);
	const idx = parts.lastIndexOf("node_modules");
	if (idx === -1) return undefined;
	return parts.slice(0, idx + 1).join(sep);
}

/**
 * Agents run their ACP adapter + SDK inside the VM from `/root/node_modules`.
 * Rather than make the caller hand-write `nodeModulesMount(...)`, auto-derive a
 * read-only `host_dir` mount of the `node_modules` root that holds the agent
 * packages (`requires`) from each agent descriptor's installed `packageDir`.
 *
 * An explicit `/root/node_modules` mount in `options.mounts` always wins. If
 * agents resolve to more than one distinct `node_modules` root the derivation is
 * ambiguous and the caller must mount explicitly.
 */
function withAutoAgentNodeModulesMount(
	mounts: NativeMountLike[] | undefined,
	descriptors: SoftwareDescriptorLike[],
): NativeMountLike[] | undefined {
	if (mounts?.some((mount) => mount.path === "/root/node_modules")) {
		return mounts;
	}

	const roots = new Set<string>();
	for (const d of descriptors) {
		if (!d.agent || typeof d.packageDir !== "string") continue;
		const root = nodeModulesRootOf(d.packageDir);
		if (root) {
			roots.add(root);
			continue;
		}
		const requires = d.requires?.length
			? ` Required packages: ${d.requires.join(", ")}.`
			: "";
		throw new Error(
			"agentOs() could not auto-mount agent node_modules: agent " +
				`packageDir ${d.packageDir} is not inside a node_modules install. ` +
				"Run from an npm-installed package tree, or pass an explicit " +
				"nodeModulesMount(<absolute node_modules path>) in options.mounts." +
				requires,
		);
	}

	if (roots.size === 0) return mounts;
	if (roots.size > 1) {
		throw new Error(
			"agentOs() could not auto-mount agent node_modules: agents resolved to " +
				`multiple node_modules roots (${[...roots].join(", ")}). Pass an ` +
				"explicit nodeModulesMount(...) in options.mounts.",
		);
	}

	const [hostNodeModulesDir] = [...roots];
	return [...(mounts ?? []), nodeModulesMount(hostNodeModulesDir)];
}

/**
 * Stable identity for a software descriptor, used to de-duplicate the
 * auto-injected default bundle against software the caller passed explicitly.
 * Resolved `commandDir`/`packageDir` paths are the most reliable key; `name`
 * is the fallback for descriptors without a directory.
 */
function softwareIdentity(d: SoftwareDescriptorLike): string {
	if (typeof d.commandDir === "string") return `dir:${d.commandDir}`;
	if (typeof d.packageDir === "string") return `dir:${d.packageDir}`;
	const name = (d as { name?: unknown }).name;
	if (typeof name === "string") return `name:${name}`;
	return JSON.stringify(d);
}

function flattenSoftware(input: unknown, out: SoftwareDescriptorLike[]): void {
	if (input == null) return;
	if (Array.isArray(input)) {
		for (const item of input) flattenSoftware(item, out);
		return;
	}
	if (typeof input === "object") out.push(input as SoftwareDescriptorLike);
}

export function buildConfigJson<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
): string {
	const options = nativeAgentOsOptionsSchema.parse(
		parsed.options ?? {},
	) as Record<string, unknown>;
	const descriptors: SoftwareDescriptorLike[] = [];
	flattenSoftware(options.software, descriptors);

	// Auto-include the default software bundle (`@agentos-software/common`: `sh` +
	// coreutils + the standard CLI tools agents rely on) unless the caller opted
	// out with `defaultSoftware: false`. Anything already listed in `software`
	// (e.g. an explicit `common`) is not duplicated. Prepended so the baseline
	// tools come first, matching the previous explicit `[common, ...]` ordering.
	const defaultSoftwareEnabled = options.defaultSoftware !== false;
	if (defaultSoftwareEnabled) {
		const defaults: SoftwareDescriptorLike[] = [];
		flattenSoftware(common, defaults);
		const seen = new Set(descriptors.map(softwareIdentity));
		const toPrepend = defaults.filter((d) => !seen.has(softwareIdentity(d)));
		descriptors.unshift(...toPrepend);
	}

	const software: Array<{ package: string; kind?: string }> = [];
	for (const d of descriptors) {
		if (typeof d.commandDir === "string") {
			// Wasm command directory (kind defaults to WasmCommands on the Rust side).
			software.push({ package: d.commandDir });
		} else if (typeof d.packageDir === "string") {
			// Agent SDK / host-tool package: forwarded but not mounted as commands.
			// `kind` matches the kebab-case serde tags of the Rust `SoftwareKind`
			// enum (`wasm-commands` / `agent` / `tool`).
			software.push({
				package: d.packageDir,
				kind: d.hostTool || d.toolkit ? "tool" : "agent",
			});
		}
	}

	// `/root/node_modules` (agent ACP adapter + SDK + transitive dep resolution)
	// is auto-derived from the agent descriptors so the standard quickstart needs
	// no manual `nodeModulesMount(...)`: see `withAutoAgentNodeModulesMount`. An
	// explicit `/root/node_modules` mount in `options.mounts` always wins. The VM
	// module resolver reads the mounted tree through the kernel VFS.
	const mounts = withAutoAgentNodeModulesMount(
		serializeNativeMounts(options.mounts),
		descriptors,
	);
	const sidecar = serializeSidecar(options.sidecar);
	return JSON.stringify({
		software,
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
		throw new Error("agentOs() options.mounts must be an array");
	}
	return input.map((mount, index) => {
		if (!mount || typeof mount !== "object") {
			throw new Error(`agentOs() options.mounts[${index}] must be an object`);
		}
		const record = mount as Record<string, unknown>;
		if (record.driver !== undefined) {
			throw new Error(
				"agentOs() only supports Native mounts across the NAPI boundary; Plain mounts with driver callbacks are not serializable",
			);
		}
		if (record.filesystem !== undefined) {
			throw new Error(
				"agentOs() only supports Native mounts across the NAPI boundary; Overlay mounts are not serializable",
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
				`agentOs() options.mounts[${index}] must be a Native mount with { path, plugin: { id, config? } }`,
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
		throw new Error("agentOs() options.sidecar must be an object");
	}
	const record = input as Record<string, unknown>;
	if (record.kind === "explicit" || record.handle !== undefined) {
		throw new Error(
			"agentOs() only supports sidecar shared pool configuration across the NAPI boundary; explicit sidecar handles are not serializable",
		);
	}
	if (record.kind !== undefined && record.kind !== "shared") {
		throw new Error('agentOs() options.sidecar.kind must be "shared"');
	}
	return typeof record.pool === "string" ? { pool: record.pool } : {};
}

function buildNativeFactoryBuilder<TConnParams>(
	parsed: AgentOsActorConfig<TConnParams>,
): (runtime: CoreRuntime) => ActorFactoryHandle {
	return (runtime) => {
		if (runtime.kind !== "napi") {
			throw new Error(
				`agentOs() is only supported on the native NAPI runtime (current runtime kind: ${runtime.kind})`,
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
		};
		return runtime.createNativePluginFactory(options);
	};
}

/**
 * Type alias for the `agentOs(...)` return type. Events are not typed at the TS
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

export function agentOs<TConnParams = undefined>(
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
	} as Parameters<
		typeof actor
	>[0]) as unknown as AgentOsActorDefinition<TConnParams>;
	definition.nativeFactoryBuilder = buildNativeFactoryBuilder(parsed);
	return definition;
}
