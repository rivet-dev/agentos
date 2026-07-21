import crypto from "node:crypto";
import {
	type AgentExitEvent,
	AgentOs,
	type AgentOsOptions,
	type CronEvent,
	type DynamicMountDescriptor,
	type OpenSessionInput,
	type PackageDescriptor,
	type PromptInput,
	type SessionStreamEntry,
} from "@rivet-dev/agentos-core";
import {
	type Actions,
	type ActorConfigInput,
	type ActorContext,
	type ActorDefinition,
	actor,
	event,
	type Type,
	UserError,
} from "rivetkit";
import { type DatabaseProvider, db, type RawAccess } from "rivetkit/db";
import { migrations } from "rivetkit/unstable/migrations";
import type {
	AgentOsEvents,
	ProcessExitPayload,
	ProcessOutputPayload,
	SerializableCronJobOptions,
	ShellDataPayload,
	ShellExitPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "./types.js";

// Prompts may remain paused at a permission request for a long human review.
// RivetKit currently exposes only one actor-wide action timeout, so use the
// largest value Node timers can represent safely (~24.8 days). Callers may
// still opt into a shorter timeout through actor options.
const DEFAULT_ACTION_TIMEOUT_MS = 2_147_483_647;
const DEFAULT_SLEEP_GRACE_PERIOD_MS = 15 * 60_000;
const DEFAULT_PREVIEW_TTL_SECONDS = 3_600;
const MAX_PREVIEW_TTL_SECONDS = 86_400;
const DEFAULT_MAX_ACTIVE_PREVIEW_TOKENS = 1_024;
const DEFAULT_MAX_SESSION_SUBSCRIPTIONS = 10_000;
const DEFAULT_MAX_DYNAMIC_MOUNTS = 10_000;
const DEFAULT_MAX_LINKED_SOFTWARE = 10_000;
const ACTOR_SQLITE_CHUNK_SIZE = 512 * 1024;
const ACTOR_SQLITE_INLINE_THRESHOLD = 64 * 1024;
const ROOT_NAMESPACE = "agentos-root";
const PREVIEW_PATH_PATTERN = /^\/fetch\/([a-f0-9]{48})(\/.*)?$/;
const MAX_SQLITE_SAFE_INTEGER = Number.MAX_SAFE_INTEGER;

interface ActorSqliteMigration {
	readonly version: number;
	readonly sql: string;
}

const ACTOR_SQLITE_MIGRATIONS = [
	{
		version: 1,
		sql: `
			CREATE TABLE agentos_actor_preview_tokens (
				token TEXT PRIMARY KEY CHECK (length(token) = 48 AND token NOT GLOB '*[^0-9a-f]*'),
				port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
				created_at_ms INTEGER NOT NULL CHECK (created_at_ms BETWEEN 0 AND ${MAX_SQLITE_SAFE_INTEGER}),
				expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > created_at_ms AND expires_at_ms <= ${MAX_SQLITE_SAFE_INTEGER})
			) STRICT;
			CREATE INDEX agentos_actor_preview_tokens_by_expiry
				ON agentos_actor_preview_tokens(expires_at_ms);
			CREATE TABLE agentos_actor_dynamic_mounts (
				path TEXT PRIMARY KEY CHECK (substr(path, 1, 1) = '/' AND instr(path, char(0)) = 0),
				descriptor_json TEXT NOT NULL CHECK (json_valid(descriptor_json) AND json_type(descriptor_json) = 'object')
			) STRICT;
			CREATE TABLE agentos_actor_linked_software (
				path TEXT PRIMARY KEY CHECK (length(path) > 0 AND instr(path, char(0)) = 0),
				descriptor_json TEXT NOT NULL CHECK (json_valid(descriptor_json) AND json_type(descriptor_json) = 'object')
			) STRICT;
		`,
	},
] as const satisfies readonly ActorSqliteMigration[];

type BuiltInEvents = {
	[K in keyof AgentOsEvents]: Type<AgentOsEvents[K]>;
};

const builtInEvents: BuiltInEvents = {
	sessionEvent: event<SessionStreamEntry>(),
	vmBooted: event<VmBootedPayload>(),
	vmShutdown: event<VmShutdownPayload>(),
	processOutput: event<ProcessOutputPayload>(),
	processExit: event<ProcessExitPayload>(),
	shellData: event<ShellDataPayload>(),
	cronEvent: event<CronEvent>(),
	agentExit: event<AgentExitEvent>(),
	shellStderr: event<ShellDataPayload>(),
	shellExit: event<ShellExitPayload>(),
};
type ActorDb = DatabaseProvider<RawAccess>;
type EventSchemaConfig = Record<string, any>;
type QueueSchemaConfig = Record<string, any>;
type AnyContext = ActorContext<any, any, any, any, any, ActorDb, any, any>;

interface RuntimeState {
	vm: Promise<AgentOs> | null;
	subscribedSessions: Map<string, readonly (() => void)[]>;
	onVmStop?: (
		c: AnyContext,
		vm: AgentOs,
		reason: "sleep" | "destroy" | "error",
	) => void | Promise<void>;
	onVmDisposed?: (
		c: AnyContext,
		reason: "sleep" | "destroy" | "error",
	) => void | Promise<void>;
	vmCleanupRan: boolean;
	fetchStreamHolds: Map<string, () => void>;
}

const runtimes = new Map<string, RuntimeState>();
const optionResolvers = new WeakMap<
	AgentOsOptions,
	(c: AnyContext) => AgentOsOptions | Promise<AgentOsOptions>
>();
const vmStartResolvers = new WeakMap<
	AgentOsOptions,
	(c: AnyContext, vm: AgentOs) => void | Promise<void>
>();
const vmStopResolvers = new WeakMap<
	AgentOsOptions,
	(
		c: AnyContext,
		vm: AgentOs,
		reason: "sleep" | "destroy" | "error",
	) => void | Promise<void>
>();
const vmDisposedResolvers = new WeakMap<
	AgentOsOptions,
	(c: AnyContext, reason: "sleep" | "destroy" | "error") => void | Promise<void>
>();

function runtimeFor(c: AnyContext): RuntimeState {
	let runtime = runtimes.get(c.actorId);
	if (!runtime) {
		runtime = {
			vm: null,
			subscribedSessions: new Map(),
			vmCleanupRan: false,
			fetchStreamHolds: new Map(),
		};
		runtimes.set(c.actorId, runtime);
	}
	return runtime;
}

async function runVmDisposedHook(
	runtime: RuntimeState,
	c: AnyContext,
	reason: "sleep" | "destroy" | "error",
): Promise<void> {
	if (runtime.vmCleanupRan) return;
	runtime.vmCleanupRan = true;
	await runtime.onVmDisposed?.(c, reason);
}

async function ensureVm(
	c: AnyContext,
	options?: AgentOsOptions,
): Promise<AgentOs> {
	const runtime = runtimeFor(c);
	if (runtime.vm !== null) return runtime.vm;

	const startedAt = Date.now();
	runtime.vmCleanupRan = false;
	runtime.vm = (async () => {
		runtime.onVmStop = options ? vmStopResolvers.get(options) : undefined;
		runtime.onVmDisposed = options
			? vmDisposedResolvers.get(options)
			: undefined;
		const resolvedOptions = {
			...options,
			...(options && optionResolvers.has(options)
				? await optionResolvers.get(options)?.(c)
				: undefined),
		};
		if (resolvedOptions.rootFilesystem) {
			throw new Error(
				"agentOS() owns rootFilesystem so it can persist directly through the actor SQLite UDS; resolveOptions may configure mounts but not rootFilesystem",
			);
		}
		if (resolvedOptions.database) {
			throw new Error(
				"agentOS() owns database and injects the actor SQLite UDS descriptor; resolveOptions cannot replace it",
			);
		}
		if (resolvedOptions.sandbox && "client" in resolvedOptions.sandbox) {
			throw new Error(
				"agentOS() cannot share sandbox: { client } across actor instances; resolveOptions must use sandbox: { provider }",
			);
		}
		const actorRuntimeSocket = (
			c as AnyContext & {
				actorRuntimeSocket(): Promise<{ path: string }>;
			}
		).actorRuntimeSocket;
		if (typeof actorRuntimeSocket !== "function") {
			throw new Error(
				"AgentOS actors require a RivetKit runtime with Actor Runtime Socket support",
			);
		}
		const { path } = await actorRuntimeSocket.call(c);
		const mountRows = await c.db.execute<{ descriptor_json: string }>(
			"SELECT descriptor_json FROM agentos_actor_dynamic_mounts ORDER BY path",
		);
		const softwareRows = await c.db.execute<{ descriptor_json: string }>(
			"SELECT descriptor_json FROM agentos_actor_linked_software ORDER BY path",
		);
		const vm = await AgentOs.create({
			...resolvedOptions,
			database: { type: "actor_uds", path },
			onAgentExit: (event) => {
				c.log.error({
					msg: "agent-os agent adapter exited unexpectedly",
					...event,
				});
				c.broadcast("agentExit", event);
				try {
					resolvedOptions.onAgentExit?.(event);
				} catch (error) {
					c.log.error({
						msg: "agent-os onAgentExit hook failed",
						sessionId: event.sessionId,
						error,
					});
				}
			},
			rootFilesystem: {
				type: "native",
				plugin: {
					id: "chunked_actor_sqlite",
					config: {
						namespace: ROOT_NAMESPACE,
						chunkSize: ACTOR_SQLITE_CHUNK_SIZE,
						inlineThreshold: ACTOR_SQLITE_INLINE_THRESHOLD,
						uid:
							resolvedOptions.user?.euid ?? resolvedOptions.user?.uid ?? 1000,
						gid:
							resolvedOptions.user?.egid ?? resolvedOptions.user?.gid ?? 1000,
					},
				},
			},
		});
		try {
			for (const row of mountRows) {
				await vm.mountFs(
					JSON.parse(row.descriptor_json) as DynamicMountDescriptor,
				);
			}
			for (const row of softwareRows) {
				await vm.linkSoftware(
					JSON.parse(row.descriptor_json) as PackageDescriptor,
				);
			}
		} catch (error) {
			await vm.dispose();
			throw error;
		}
		const onVmStart = options ? vmStartResolvers.get(options) : undefined;
		if (onVmStart) {
			try {
				await onVmStart(c, vm);
			} catch (error) {
				try {
					await vm.dispose();
				} catch (disposeError) {
					c.log.error({
						msg: "failed to dispose AgentOS VM after start hook failure",
						disposeError,
					});
				} finally {
					await runVmDisposedHook(runtime, c, "error");
				}
				throw error;
			}
		}
		vm.onCronEvent((cronEvent) => c.broadcast("cronEvent", cronEvent));
		c.broadcast("vmBooted", {});
		c.log.info({
			msg: "agent-os vm booted",
			bootDurationMs: Date.now() - startedAt,
		});
		return vm;
	})();

	try {
		return await runtime.vm;
	} catch (error) {
		runtime.vm = null;
		c.log.error({
			msg: "agent-os vm bootstrap failed",
			actorId: c.actorId,
			error,
		});
		await runVmDisposedHook(runtime, c, "error");
		throw error;
	}
}

async function disposeVm(c: AnyContext, reason: "sleep" | "destroy" | "error") {
	const runtime = runtimes.get(c.actorId);
	if (!runtime) return;
	const vm = runtime.vm;
	runtimes.delete(c.actorId);
	for (const unsubscribers of runtime.subscribedSessions.values()) {
		for (const unsubscribe of unsubscribers) unsubscribe();
	}
	runtime.subscribedSessions.clear();
	for (const release of runtime.fetchStreamHolds.values()) release();
	runtime.fetchStreamHolds.clear();
	if (vm) {
		const failures: unknown[] = [];
		try {
			const resolvedVm = await vm;
			try {
				await runtime.onVmStop?.(c, resolvedVm, reason);
			} catch (error) {
				failures.push(error);
			}
			try {
				await resolvedVm.dispose();
			} catch (error) {
				failures.push(error);
			}
		} catch (error) {
			failures.push(error);
		}
		try {
			await runVmDisposedHook(runtime, c, reason);
		} catch (error) {
			failures.push(error);
		}
		if (failures.length === 1) throw failures[0];
		if (failures.length > 1) {
			const aggregate = new Error(
				"failed to stop, dispose, or clean up AgentOS VM",
			);
			(aggregate as Error & { errors: unknown[] }).errors = failures;
			throw aggregate;
		}
	}
	c.broadcast("vmShutdown", { reason });
}

function matchPreviewPath(pathname: string): RegExpMatchArray | null {
	return pathname.match(PREVIEW_PATH_PATTERN);
}

/** @internal Exported only for focused migration-ladder tests. */
export function validateAgentOsActorMigrationLadder(
	migrations: readonly ActorSqliteMigration[],
): void {
	if (migrations.length === 0) {
		throw new Error("AgentOS actor SQLite migration ladder must not be empty");
	}
	for (const [index, migration] of migrations.entries()) {
		const expectedVersion = index + 1;
		if (migration.version !== expectedVersion) {
			throw new Error(
				`invalid AgentOS actor SQLite migration ladder: expected version ${expectedVersion}, received ${migration.version}`,
			);
		}
		if (migration.sql.trim().length === 0) {
			throw new Error(
				`invalid AgentOS actor SQLite migration ${migration.version}: SQL is empty`,
			);
		}
		if (
			/(?:^|;)\s*(?:BEGIN|COMMIT|ROLLBACK|SAVEPOINT|RELEASE)\b/im.test(
				migration.sql,
			)
		) {
			throw new Error(
				`invalid AgentOS actor SQLite migration ${migration.version}: transaction control belongs to the migration provider`,
			);
		}
		if (/\b(?:FOREIGN\s+KEY|REFERENCES)\b/i.test(migration.sql)) {
			throw new Error(
				`invalid AgentOS actor SQLite migration ${migration.version}: foreign keys are not used in the actor-owned schema`,
			);
		}
		for (const statement of migration.sql.split(";")) {
			if (
				/^\s*CREATE\s+TABLE\b/i.test(statement) &&
				!/[)]\s*STRICT\s*$/i.test(statement)
			) {
				throw new Error(
					`invalid AgentOS actor SQLite migration ${migration.version}: every actor-owned table must be STRICT`,
				);
			}
		}
		const ownedIdentifiers = migration.sql.match(/\bagentos_[a-z0-9_]+\b/gi);
		if (
			!ownedIdentifiers ||
			ownedIdentifiers.some(
				(identifier) => !identifier.startsWith("agentos_actor_"),
			)
		) {
			throw new Error(
				`invalid AgentOS actor SQLite migration ${migration.version}: migrations may reference only agentos_actor_* tables and indexes`,
			);
		}
	}
}

/** @internal Exported only for focused migration tests. */
export async function migrateAgentOsActorTables(
	database: RawAccess,
): Promise<void> {
	validateAgentOsActorMigrationLadder(ACTOR_SQLITE_MIGRATIONS);
	await runAgentOsActorMigrations(database);
}

const runAgentOsActorMigrations = migrations({
	tableName: "agentos_actor_schema_version",
	migrations: ACTOR_SQLITE_MIGRATIONS,
});

async function assertActorCollectionCapacity(
	c: AnyContext,
	table: "agentos_actor_dynamic_mounts" | "agentos_actor_linked_software",
	key: string,
	limit: number,
	label: "dynamic mounts" | "linked software packages",
	configKey: "maxDynamicMounts" | "maxLinkedSoftware",
): Promise<void> {
	const existing = await c.db.execute<{ present: number }>(
		`SELECT 1 AS present FROM ${table} WHERE path = ? LIMIT 1`,
		key,
	);
	if (existing.length > 0) return;
	const rows = await c.db.execute<{ count: number }>(
		`SELECT COUNT(*) AS count FROM ${table}`,
	);
	const count = Number(rows[0]?.count ?? 0);
	if (count >= limit) {
		throw new UserError(
			`${label} limit ${limit} reached; raise ${configKey} to persist more`,
			{
				code: `agentos_${configKey === "maxDynamicMounts" ? "dynamic_mount" : "linked_software"}_limit`,
				metadata: { limit },
			},
		);
	}
	if (count + 1 === Math.ceil(limit * 0.8)) {
		c.log.warn({
			msg: `${label} are near the limit of ${limit}; raise ${configKey} to persist more`,
			count: count + 1,
			limit,
		});
	}
}

export interface VmFetchOptions {
	method?: string;
	headers?: Record<string, string>;
	body?: string | Uint8Array;
}

export interface VmFetchResponse {
	status: number;
	statusText: string;
	headers: Record<string, string>;
	rawHeaders?: Array<[string, string]>;
	body: Uint8Array;
}

export interface VmFetchStreamHead {
	streamId: string;
	status: number;
	statusText: string;
	headers: Record<string, string>;
	rawHeaders?: Array<[string, string]>;
}

export interface VmFetchStreamChunk {
	body: Uint8Array;
	done: boolean;
}

export interface AgentOsEventHooks<TContext = AnyContext> {
	onSessionEvent?: (
		c: TContext,
		sessionId: string,
		event: SessionStreamEntry,
	) => void | Promise<void>;
}

function trackSessionEvents(
	c: AnyContext,
	vm: AgentOs,
	sessionId: string,
	hooks: AgentOsEventHooks,
	maxSessionSubscriptions: number,
): void {
	const runtime = runtimeFor(c);
	if (runtime.subscribedSessions.has(sessionId)) return;
	if (runtime.subscribedSessions.size >= maxSessionSubscriptions) {
		throw new UserError(
			`session subscription limit ${maxSessionSubscriptions} reached; raise maxSessionSubscriptions to observe more sessions`,
			{
				code: "agentos_session_subscription_limit",
				metadata: { limit: maxSessionSubscriptions },
			},
		);
	}
	const nextCount = runtime.subscribedSessions.size + 1;
	if (nextCount === Math.ceil(maxSessionSubscriptions * 0.8)) {
		c.log.warn({
			msg: `session subscriptions are near the limit of ${maxSessionSubscriptions}; raise maxSessionSubscriptions to observe more sessions`,
			subscriptionCount: nextCount,
			limit: maxSessionSubscriptions,
		});
	}
	const unsubscribeSession = vm.onSessionEvent(
		sessionId,
		(notification: SessionStreamEntry) => {
			const serialized = JSON.parse(
				JSON.stringify(notification),
			) as SessionStreamEntry;
			c.broadcast("sessionEvent", serialized);
			if (hooks.onSessionEvent) {
				void c.keepAwake(
					Promise.resolve()
						.then(() => hooks.onSessionEvent?.(c, sessionId, serialized))
						.catch((error) =>
							c.log.error({
								msg: "agent-os session event hook failed",
								sessionId,
								error,
							}),
						),
				);
			}
		},
	);
	runtime.subscribedSessions.set(sessionId, [unsubscribeSession]);
}

function untrackSessionEvents(c: AnyContext, sessionId: string): void {
	const unsubscribers = runtimeFor(c).subscribedSessions.get(sessionId);
	if (!unsubscribers) return;
	for (const unsubscribe of unsubscribers) unsubscribe();
	runtimeFor(c).subscribedSessions.delete(sessionId);
}

function trackFetchStream(c: AnyContext, streamId: string): void {
	const runtime = runtimeFor(c);
	if (runtime.fetchStreamHolds.has(streamId)) return;
	const hold = new Promise<void>((resolve) => {
		runtime.fetchStreamHolds.set(streamId, resolve);
	});
	void c.keepAwake(hold).catch((error) =>
		c.log.error({
			msg: "agent-os fetch stream hold failed",
			streamId,
			error,
		}),
	);
}

function releaseFetchStream(c: AnyContext, streamId: string): void {
	const runtime = runtimeFor(c);
	runtime.fetchStreamHolds.get(streamId)?.();
	runtime.fetchStreamHolds.delete(streamId);
}

export function createAgentOsActions(
	options?: AgentOsOptions,
	hooks: AgentOsEventHooks = {},
	preview: AgentOsActorExtras["preview"] = {},
	maxSessionSubscriptions = DEFAULT_MAX_SESSION_SUBSCRIPTIONS,
	maxDynamicMounts = DEFAULT_MAX_DYNAMIC_MOUNTS,
	maxLinkedSoftware = DEFAULT_MAX_LINKED_SOFTWARE,
) {
	const defaultPreviewTtlSeconds =
		preview.defaultExpiresInSeconds ?? DEFAULT_PREVIEW_TTL_SECONDS;
	const maxPreviewTtlSeconds =
		preview.maxExpiresInSeconds ?? MAX_PREVIEW_TTL_SECONDS;
	const maxActivePreviewTokens =
		preview.maxActiveTokens ?? DEFAULT_MAX_ACTIVE_PREVIEW_TOKENS;
	if (
		!Number.isFinite(defaultPreviewTtlSeconds) ||
		defaultPreviewTtlSeconds <= 0 ||
		!Number.isFinite(maxPreviewTtlSeconds) ||
		maxPreviewTtlSeconds <= 0 ||
		defaultPreviewTtlSeconds > maxPreviewTtlSeconds
	) {
		throw new UserError(
			"preview expiry bounds must be positive and the default cannot exceed the maximum",
			{ code: "agentos_preview_invalid_config" },
		);
	}
	if (!Number.isInteger(maxActivePreviewTokens) || maxActivePreviewTokens < 1) {
		throw new UserError("preview.maxActiveTokens must be a positive integer", {
			code: "agentos_preview_invalid_config",
		});
	}
	if (
		!Number.isInteger(maxSessionSubscriptions) ||
		maxSessionSubscriptions < 1
	) {
		throw new UserError("maxSessionSubscriptions must be a positive integer", {
			code: "agentos_session_subscription_invalid_config",
		});
	}
	if (!Number.isInteger(maxDynamicMounts) || maxDynamicMounts < 1) {
		throw new UserError("maxDynamicMounts must be a positive integer", {
			code: "agentos_dynamic_mount_invalid_config",
		});
	}
	if (!Number.isInteger(maxLinkedSoftware) || maxLinkedSoftware < 1) {
		throw new UserError("maxLinkedSoftware must be a positive integer", {
			code: "agentos_linked_software_invalid_config",
		});
	}
	return {
		readFile: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readFile"]>
		) => {
			try {
				return await (await ensureVm(c, options)).readFile(...args);
			} catch (error) {
				c.log.error({
					msg: "agent-os readFile action failed",
					path: args[0],
					error,
				});
				throw error;
			}
		},
		writeFile: async (
			c: AnyContext,
			...args: Parameters<AgentOs["writeFile"]>
		) => (await ensureVm(c, options)).writeFile(...args),
		readFiles: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readFiles"]>
		) => (await ensureVm(c, options)).readFiles(...args),
		writeFiles: async (
			c: AnyContext,
			...args: Parameters<AgentOs["writeFiles"]>
		) => (await ensureVm(c, options)).writeFiles(...args),
		stat: async (c: AnyContext, ...args: Parameters<AgentOs["stat"]>) =>
			(await ensureVm(c, options)).stat(...args),
		mkdir: async (c: AnyContext, ...args: Parameters<AgentOs["mkdir"]>) =>
			(await ensureVm(c, options)).mkdir(...args),
		readdir: async (c: AnyContext, ...args: Parameters<AgentOs["readdir"]>) =>
			(await ensureVm(c, options)).readdir(...args),
		readdirEntries: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readdirEntries"]>
		) => (await ensureVm(c, options)).readdirEntries(...args),
		readdirRecursive: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readdirRecursive"]>
		) => (await ensureVm(c, options)).readdirRecursive(...args),
		exists: async (c: AnyContext, ...args: Parameters<AgentOs["exists"]>) =>
			(await ensureVm(c, options)).exists(...args),
		move: async (c: AnyContext, ...args: Parameters<AgentOs["move"]>) =>
			(await ensureVm(c, options)).move(...args),
		remove: async (c: AnyContext, ...args: Parameters<AgentOs["remove"]>) =>
			(await ensureVm(c, options)).remove(...args),
		exec: async (c: AnyContext, ...args: Parameters<AgentOs["exec"]>) =>
			(await ensureVm(c, options)).exec(...args),
		execArgv: async (c: AnyContext, ...args: Parameters<AgentOs["execArgv"]>) =>
			(await ensureVm(c, options)).execArgv(...args),
		spawn: async (c: AnyContext, ...args: Parameters<AgentOs["spawn"]>) => {
			const vm = await ensureVm(c, options);
			const process = vm.spawn(...args);
			const unsubscribeOutput = vm.onProcessOutput(process.pid, (event) =>
				c.broadcast("processOutput", event),
			);
			const unsubscribeExit = vm.onProcessExit(process.pid, (event) =>
				c.broadcast("processExit", event),
			);
			void c
				.keepAwake(
					vm.waitProcess(process.pid).finally(() => {
						unsubscribeOutput();
						unsubscribeExit();
					}),
				)
				.catch((error) =>
					c.log.error({
						msg: "agent-os process wait failed",
						pid: process.pid,
						error,
					}),
				);
			return process;
		},
		waitProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["waitProcess"]>
		) => (await ensureVm(c, options)).waitProcess(...args),
		killProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["killProcess"]>
		) => (await ensureVm(c, options)).killProcess(...args),
		stopProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["stopProcess"]>
		) => (await ensureVm(c, options)).stopProcess(...args),
		listProcesses: async (c: AnyContext) =>
			(await ensureVm(c, options)).listProcesses(),
		allProcesses: async (c: AnyContext) =>
			(await ensureVm(c, options)).allProcesses(),
		processTree: async (c: AnyContext) =>
			(await ensureVm(c, options)).processTree(),
		getProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["getProcess"]>
		) => (await ensureVm(c, options)).getProcess(...args),
		writeProcessStdin: async (
			c: AnyContext,
			...args: Parameters<AgentOs["writeProcessStdin"]>
		) => (await ensureVm(c, options)).writeProcessStdin(...args),
		closeProcessStdin: async (
			c: AnyContext,
			...args: Parameters<AgentOs["closeProcessStdin"]>
		) => (await ensureVm(c, options)).closeProcessStdin(...args),
		openShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["openShell"]>
		) => {
			const vm = await ensureVm(c, options);
			const shell = vm.openShell(...args);
			const unsubscribeData = vm.onShellData(shell.shellId, (event) =>
				c.broadcast("shellData", event),
			);
			const unsubscribeStderr = vm.onShellStderr(shell.shellId, (event) =>
				c.broadcast("shellStderr", event),
			);
			const unsubscribeExit = vm.onShellExit(shell.shellId, (event) =>
				c.broadcast("shellExit", event),
			);
			void c
				.keepAwake(
					vm.waitShell(shell.shellId).finally(() => {
						unsubscribeData();
						unsubscribeStderr();
						unsubscribeExit();
					}),
				)
				.catch((error) =>
					c.log.error({
						msg: "agent-os shell wait failed",
						shellId: shell.shellId,
						error,
					}),
				);
			return shell;
		},
		writeShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["writeShell"]>
		) => (await ensureVm(c, options)).writeShell(...args),
		resizeShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["resizeShell"]>
		) => (await ensureVm(c, options)).resizeShell(...args),
		closeShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["closeShell"]>
		) => (await ensureVm(c, options)).closeShell(...args),
		waitShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["waitShell"]>
		) => (await ensureVm(c, options)).waitShell(...args),
		httpRequest: async (
			c: AnyContext,
			...args: Parameters<AgentOs["httpRequest"]>
		) => (await ensureVm(c, options)).httpRequest(...args),
		vmFetch: async (
			c: AnyContext,
			port: number,
			url: string,
			requestOptions?: VmFetchOptions,
		): Promise<VmFetchResponse> => {
			const vm = await ensureVm(c, options);
			const body =
				requestOptions?.body instanceof Uint8Array
					? Buffer.from(requestOptions.body)
					: requestOptions?.body;
			const response = await vm.fetch(
				port,
				new Request(url, {
					method: requestOptions?.method ?? "GET",
					headers: requestOptions?.headers,
					body,
				}),
			);
			const headers: Record<string, string> = {};
			const rawHeaders: Array<[string, string]> = [];
			response.headers.forEach((value, key) => {
				headers[key] = value;
				if (key !== "set-cookie") rawHeaders.push([key, value]);
			});
			for (const value of response.headers.getSetCookie?.() ?? []) {
				rawHeaders.push(["set-cookie", value]);
			}
			return {
				status: response.status,
				statusText: response.statusText,
				headers,
				rawHeaders,
				body: new Uint8Array(await response.arrayBuffer()),
			};
		},
		vmFetchStreamStart: async (
			c: AnyContext,
			port: number,
			url: string,
			requestOptions?: VmFetchOptions,
		): Promise<VmFetchStreamHead> => {
			const vm = await ensureVm(c, options);
			const body =
				requestOptions?.body instanceof Uint8Array
					? Buffer.from(requestOptions.body)
					: requestOptions?.body;
			const head = await vm.fetchStreamStart(
				port,
				new Request(url, {
					method: requestOptions?.method ?? "GET",
					headers: requestOptions?.headers,
					body,
				}),
			);
			trackFetchStream(c, head.streamId);
			const headers: Record<string, string> = {};
			for (const [name, value] of head.headers) headers[name] = value;
			return { ...head, headers, rawHeaders: head.headers };
		},
		vmFetchStreamRead: async (
			c: AnyContext,
			streamId: string,
			maxBytes?: number,
		): Promise<VmFetchStreamChunk> => {
			try {
				const chunk = await (await ensureVm(c, options)).fetchStreamRead(
					streamId,
					maxBytes,
				);
				if (chunk.done) releaseFetchStream(c, streamId);
				return chunk;
			} catch (error) {
				releaseFetchStream(c, streamId);
				throw error;
			}
		},
		vmFetchStreamCancel: async (c: AnyContext, streamId: string) => {
			try {
				await (await ensureVm(c, options)).fetchStreamCancel(streamId);
			} finally {
				releaseFetchStream(c, streamId);
			}
		},
		scheduleCron: async (
			c: AnyContext,
			cronOptions: SerializableCronJobOptions,
		) => {
			const job = (await ensureVm(c, options)).scheduleCron(
				cronOptions as Parameters<AgentOs["scheduleCron"]>[0],
			);
			return { id: job.id };
		},
		listCronJobs: async (c: AnyContext) =>
			(await ensureVm(c, options)).listCronJobs(),
		cancelCronJob: async (
			c: AnyContext,
			...args: Parameters<AgentOs["cancelCronJob"]>
		) => (await ensureVm(c, options)).cancelCronJob(...args),
		listAgents: async (c: AnyContext) =>
			(await ensureVm(c, options)).listAgents(),
		openSession: async (c: AnyContext, input: OpenSessionInput) => {
			const vm = await ensureVm(c, options);
			try {
				await vm.openSession(input);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				const causeCode =
					typeof (error as { code?: unknown })?.code === "string"
						? (error as { code: string }).code
						: undefined;
				c.log.error({
					msg: "agent-os openSession action failed",
					sessionId: input.sessionId ?? "main",
					agent: input.agent,
					causeCode,
					error,
				});
				throw new UserError(`AgentOS openSession failed: ${message}`, {
					code: "agentos_session_open_failed",
					metadata: causeCode ? { causeCode } : undefined,
				});
			}
			trackSessionEvents(
				c,
				vm,
				input.sessionId ?? "main",
				hooks,
				maxSessionSubscriptions,
			);
		},
		getSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["getSession"]>
		) => (await ensureVm(c, options)).getSession(...args),
		listSessions: async (
			c: AnyContext,
			...args: Parameters<AgentOs["listSessions"]>
		) => (await ensureVm(c, options)).listSessions(...args),
		deleteSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["deleteSession"]>
		) => {
			const result = await (await ensureVm(c, options)).deleteSession(...args);
			untrackSessionEvents(c, args[0]?.sessionId ?? "main");
			return result;
		},
		unloadSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["unloadSession"]>
		) => {
			const result = await (await ensureVm(c, options)).unloadSession(...args);
			untrackSessionEvents(c, args[0]?.sessionId ?? "main");
			return result;
		},
		prompt: async (c: AnyContext, input: PromptInput) => {
			const vm = await ensureVm(c, options);
			const sessionId = input.sessionId ?? "main";
			trackSessionEvents(c, vm, sessionId, hooks, maxSessionSubscriptions);
			// The actor is held only through the terminal SQLite commit for this
			// active turn. Merely having an idle durable session holds nothing.
			try {
				return await c.keepAwake(vm.prompt(input));
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				const causeCode =
					typeof (error as { code?: unknown })?.code === "string"
						? (error as { code: string }).code
						: undefined;
				c.log.error({
					msg: "agent-os prompt action failed",
					sessionId,
					causeCode,
					error,
				});
				throw new UserError(`AgentOS prompt failed: ${message}`, {
					code: "agentos_prompt_failed",
					metadata: causeCode ? { causeCode } : undefined,
				});
			}
		},
		cancelPrompt: async (
			c: AnyContext,
			...args: Parameters<AgentOs["cancelPrompt"]>
		) => (await ensureVm(c, options)).cancelPrompt(...args),
		respondPermission: async (
			c: AnyContext,
			...args: Parameters<AgentOs["respondPermission"]>
		) => (await ensureVm(c, options)).respondPermission(...args),
		readHistory: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readHistory"]>
		) => (await ensureVm(c, options)).readHistory(...args),
		getSessionConfig: async (
			c: AnyContext,
			...args: Parameters<AgentOs["getSessionConfig"]>
		) => (await ensureVm(c, options)).getSessionConfig(...args),
		setSessionConfigOption: async (
			c: AnyContext,
			...args: Parameters<AgentOs["setSessionConfigOption"]>
		) => (await ensureVm(c, options)).setSessionConfigOption(...args),
		getSessionCapabilities: async (
			c: AnyContext,
			...args: Parameters<AgentOs["getSessionCapabilities"]>
		) => (await ensureVm(c, options)).getSessionCapabilities(...args),
		getSessionAgentInfo: async (
			c: AnyContext,
			...args: Parameters<AgentOs["getSessionAgentInfo"]>
		) => (await ensureVm(c, options)).getSessionAgentInfo(...args),
		createPreviewUrl: async (
			c: AnyContext,
			port: number,
			ttlSeconds = defaultPreviewTtlSeconds,
		) => {
			if (!Number.isInteger(port) || port < 1 || port > 65_535)
				throw new UserError(
					"port must be an integer between 1 and 65535; pass a valid VM listener port",
					{ code: "agentos_preview_invalid_port" },
				);
			if (
				!Number.isFinite(ttlSeconds) ||
				ttlSeconds <= 0 ||
				ttlSeconds > maxPreviewTtlSeconds
			)
				throw new UserError(
					`ttlSeconds must be greater than 0 and at most ${maxPreviewTtlSeconds}; raise preview.maxExpiresInSeconds to allow a longer lifetime`,
					{ code: "agentos_preview_invalid_ttl" },
				);
			const token = crypto.randomBytes(24).toString("hex");
			const createdAt = Date.now();
			const expiresAt = createdAt + ttlSeconds * 1_000;
			await c.db.execute(
				"DELETE FROM agentos_actor_preview_tokens WHERE expires_at_ms <= ?",
				createdAt,
			);
			const counts = await c.db.execute<{ count: number }>(
				"SELECT COUNT(*) AS count FROM agentos_actor_preview_tokens",
			);
			const activeTokenCount = Number(counts[0]?.count ?? 0);
			if (activeTokenCount >= maxActivePreviewTokens) {
				throw new UserError(
					`preview token limit ${maxActivePreviewTokens} reached; raise preview.maxActiveTokens to allow more`,
					{
						code: "agentos_preview_token_limit",
						metadata: { limit: maxActivePreviewTokens },
					},
				);
			}
			const nextActiveTokenCount = activeTokenCount + 1;
			const warningThreshold = Math.ceil(maxActivePreviewTokens * 0.8);
			if (nextActiveTokenCount === warningThreshold) {
				c.log.warn({
					msg: `preview tokens are near the limit of ${maxActivePreviewTokens}; raise preview.maxActiveTokens to allow more`,
					activeTokenCount: nextActiveTokenCount,
					limit: maxActivePreviewTokens,
				});
			}
			await c.db.execute(
				"INSERT INTO agentos_actor_preview_tokens (token, port, created_at_ms, expires_at_ms) VALUES (?, ?, ?, ?)",
				token,
				port,
				createdAt,
				expiresAt,
			);
			return { path: `/fetch/${token}`, token, port, expiresAt };
		},
		expirePreviewUrl: async (c: AnyContext, token: string) => {
			await c.db.execute(
				"DELETE FROM agentos_actor_preview_tokens WHERE token = ?",
				token,
			);
		},
		exportRootFilesystem: async (
			c: AnyContext,
			...args: Parameters<AgentOs["exportRootFilesystem"]>
		) => (await ensureVm(c, options)).exportRootFilesystem(...args),
		mountFs: async (c: AnyContext, descriptor: DynamicMountDescriptor) => {
			await assertActorCollectionCapacity(
				c,
				"agentos_actor_dynamic_mounts",
				descriptor.path,
				maxDynamicMounts,
				"dynamic mounts",
				"maxDynamicMounts",
			);
			const vm = await ensureVm(c, options);
			await vm.mountFs(descriptor);
			try {
				await c.db.execute(
					"INSERT INTO agentos_actor_dynamic_mounts (path, descriptor_json) VALUES (?, ?) ON CONFLICT(path) DO UPDATE SET descriptor_json = excluded.descriptor_json",
					descriptor.path,
					JSON.stringify(descriptor),
				);
			} catch (error) {
				try {
					await vm.unmountFs(descriptor.path);
				} catch (rollbackError) {
					c.log.error({
						msg: "agent-os dynamic mount rollback failed",
						path: descriptor.path,
						error: rollbackError,
					});
				}
				throw error;
			}
		},
		unmountFs: async (c: AnyContext, path: string) => {
			const rows = await c.db.execute<{ descriptor_json: string }>(
				"SELECT descriptor_json FROM agentos_actor_dynamic_mounts WHERE path = ?",
				path,
			);
			const vm = await ensureVm(c, options);
			await vm.unmountFs(path);
			try {
				await c.db.execute(
					"DELETE FROM agentos_actor_dynamic_mounts WHERE path = ?",
					path,
				);
			} catch (error) {
				if (rows[0]) {
					try {
						await vm.mountFs(
							JSON.parse(rows[0].descriptor_json) as DynamicMountDescriptor,
						);
					} catch (rollbackError) {
						c.log.error({
							msg: "agent-os dynamic unmount rollback failed",
							path,
							error: rollbackError,
						});
					}
				}
				throw error;
			}
		},
		listMounts: async (c: AnyContext) =>
			(await ensureVm(c, options)).listMounts(),
		listSoftware: async (c: AnyContext) =>
			(await ensureVm(c, options)).listSoftware(),
		linkSoftware: async (c: AnyContext, descriptor: PackageDescriptor) => {
			await assertActorCollectionCapacity(
				c,
				"agentos_actor_linked_software",
				descriptor.path,
				maxLinkedSoftware,
				"linked software packages",
				"maxLinkedSoftware",
			);
			await c.db.execute(
				"INSERT INTO agentos_actor_linked_software (path, descriptor_json) VALUES (?, ?) ON CONFLICT(path) DO UPDATE SET descriptor_json = excluded.descriptor_json",
				descriptor.path,
				JSON.stringify(descriptor),
			);
			try {
				await (await ensureVm(c, options)).linkSoftware(descriptor);
			} catch (error) {
				try {
					await c.db.execute(
						"DELETE FROM agentos_actor_linked_software WHERE path = ?",
						descriptor.path,
					);
				} catch (rollbackError) {
					c.log.error({
						msg: "agent-os linked software rollback failed",
						path: descriptor.path,
						error: rollbackError,
					});
				}
				throw error;
			}
		},
	};
}

export type AgentOsActions = ReturnType<typeof createAgentOsActions>;
export type AgentOsActorDefinition<TConnParams = undefined> = ActorDefinition<
	undefined,
	TConnParams,
	undefined,
	undefined,
	undefined,
	ActorDb,
	BuiltInEvents,
	Record<never, never>,
	AgentOsActions
>;

export interface AgentOsActorExtras extends AgentOsOptions {
	/**
	 * Resolve trusted VM options from actor state immediately before the VM's
	 * first boot. Evaluated once per wake. The actor-owned root filesystem and
	 * database cannot be replaced.
	 */
	resolveOptions?: (c: AnyContext) => AgentOsOptions | Promise<AgentOsOptions>;
	/** Start long-lived processes after each VM boot. Trusted host code only. */
	onVmStart?: (c: AnyContext, vm: AgentOs) => void | Promise<void>;
	/** Gracefully stop long-lived processes before the VM is disposed. */
	onVmStop?: (
		c: AnyContext,
		vm: AgentOs,
		reason: "sleep" | "destroy" | "error",
	) => void | Promise<void>;
	/** Clean up host resources after the VM and lazy mount readers close. */
	onVmDisposed?: (
		c: AnyContext,
		reason: "sleep" | "destroy" | "error",
	) => void | Promise<void>;
	/** Maximum live session event subscriptions per actor VM. Default: 10,000. */
	maxSessionSubscriptions?: number;
	/** Maximum durable dynamic mount descriptors per actor. Default: 10,000. */
	maxDynamicMounts?: number;
	/** Maximum durable linked software descriptors per actor. Default: 10,000. */
	maxLinkedSoftware?: number;
	preview?: {
		defaultExpiresInSeconds?: number;
		maxExpiresInSeconds?: number;
		maxActiveTokens?: number;
	};
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown
	? Omit<T, K>
	: never;

export type AgentOsActorConfigInput<
	TState = undefined,
	TConnParams = undefined,
	TConnState = undefined,
	TVars = undefined,
	TInput = undefined,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
	TUserActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		ActorDb,
		TEvents,
		TQueues
	> = Record<never, never>,
> = DistributiveOmit<
	ActorConfigInput<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		ActorDb,
		TEvents,
		TQueues,
		TUserActions
	>,
	"db"
> &
	AgentOsActorExtras &
	AgentOsEventHooks<
		ActorContext<
			TState,
			TConnParams,
			TConnState,
			TVars,
			TInput,
			ActorDb,
			TEvents,
			TQueues
		>
	>;

const agentOsOptionKeys = [
	"software",
	"defaultSoftware",
	"loopbackExemptPorts",
	"allowedNodeBuiltins",
	"highResolutionTime",
	"wasmBackend",
	"database",
	"rootFilesystem",
	"mounts",
	"sandbox",
	"scheduleDriver",
	"bindings",
	"permissions",
	"sidecar",
	"limits",
	"onAgentStderr",
	"onAgentExit",
	"onLimitWarning",
] as const satisfies readonly (keyof AgentOsOptions)[];

function splitConfig(
	config: AgentOsActorConfigInput<any, any, any, any, any, any, any, any>,
) {
	const actorConfig = { ...config } as Record<string, unknown>;
	const agentOsOptions: AgentOsOptions = {};
	for (const key of agentOsOptionKeys) {
		if (key in actorConfig) {
			(agentOsOptions as Record<string, unknown>)[key] = actorConfig[key];
			delete actorConfig[key];
		}
	}
	const onSessionEvent =
		actorConfig.onSessionEvent as AgentOsEventHooks<AnyContext>["onSessionEvent"];
	const preview = actorConfig.preview as AgentOsActorExtras["preview"];
	const resolveOptions = actorConfig.resolveOptions as
		| AgentOsActorExtras["resolveOptions"]
		| undefined;
	const onVmStart = actorConfig.onVmStart as AgentOsActorExtras["onVmStart"];
	const onVmStop = actorConfig.onVmStop as AgentOsActorExtras["onVmStop"];
	const onVmDisposed =
		actorConfig.onVmDisposed as AgentOsActorExtras["onVmDisposed"];
	const maxSessionSubscriptions = actorConfig.maxSessionSubscriptions as
		| number
		| undefined;
	const maxDynamicMounts = actorConfig.maxDynamicMounts as number | undefined;
	const maxLinkedSoftware = actorConfig.maxLinkedSoftware as number | undefined;
	delete actorConfig.onSessionEvent;
	delete actorConfig.preview;
	delete actorConfig.resolveOptions;
	delete actorConfig.onVmStart;
	delete actorConfig.onVmStop;
	delete actorConfig.onVmDisposed;
	delete actorConfig.maxSessionSubscriptions;
	delete actorConfig.maxDynamicMounts;
	delete actorConfig.maxLinkedSoftware;
	return {
		actorConfig,
		agentOsOptions,
		hooks: { onSessionEvent },
		preview,
		resolveOptions,
		onVmStart,
		onVmStop,
		onVmDisposed,
		maxSessionSubscriptions,
		maxDynamicMounts,
		maxLinkedSoftware,
	};
}

function assertNoReservedKeys(
	kind: string,
	custom: object | undefined,
	builtIns: object,
) {
	for (const key of Object.keys(custom ?? {})) {
		if (key in builtIns)
			throw new Error(`agentOS() ${kind} name is reserved: ${key}`);
	}
}

export function createAgentOS<
	TState = undefined,
	TConnParams = undefined,
	TConnState = undefined,
	TVars = undefined,
	TInput = undefined,
	TEvents extends EventSchemaConfig = Record<never, never>,
	TQueues extends QueueSchemaConfig = Record<never, never>,
	TUserActions extends Actions<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		ActorDb,
		TEvents,
		TQueues
	> = Record<never, never>,
>(
	config: AgentOsActorConfigInput<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TEvents,
		TQueues,
		TUserActions
	> = {} as AgentOsActorConfigInput<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		TEvents,
		TQueues,
		TUserActions
	>,
): ActorDefinition<
	TState,
	TConnParams,
	TConnState,
	TVars,
	TInput,
	ActorDb,
	TEvents & BuiltInEvents,
	TQueues,
	TUserActions & AgentOsActions
> {
	const split = splitConfig(config);
	const actorConfig = split.actorConfig as Omit<
		typeof config,
		keyof AgentOsActorExtras
	>;
	const {
		agentOsOptions,
		hooks,
		preview,
		maxSessionSubscriptions,
		maxDynamicMounts,
		maxLinkedSoftware,
	} = split;
	if (split.resolveOptions)
		optionResolvers.set(agentOsOptions, split.resolveOptions);
	if (split.onVmStart) vmStartResolvers.set(agentOsOptions, split.onVmStart);
	if (split.onVmStop) vmStopResolvers.set(agentOsOptions, split.onVmStop);
	if (split.onVmDisposed)
		vmDisposedResolvers.set(agentOsOptions, split.onVmDisposed);
	if (agentOsOptions.sandbox && "client" in agentOsOptions.sandbox) {
		throw new Error(
			"agentOS() cannot share sandbox: { client } across actor instances; use sandbox: { provider } so each actor VM starts its own client",
		);
	}
	if (agentOsOptions.rootFilesystem) {
		throw new Error(
			"agentOS() owns rootFilesystem so it can persist directly through the actor SQLite UDS; use mounts for additional filesystems",
		);
	}
	if (agentOsOptions.database) {
		throw new Error(
			"agentOS() owns database and injects the actor SQLite UDS descriptor; standalone AgentOs clients may choose a SQLite file",
		);
	}
	const actions = createAgentOsActions(
		agentOsOptions,
		hooks,
		preview,
		maxSessionSubscriptions,
		maxDynamicMounts,
		maxLinkedSoftware,
	);
	assertNoReservedKeys("action", actorConfig.actions, actions);
	assertNoReservedKeys("event", actorConfig.events, builtInEvents);

	const userOnWake = actorConfig.onWake;
	const userOnSleep = actorConfig.onSleep;
	const userOnDestroy = actorConfig.onDestroy;
	const userOnRequest = actorConfig.onRequest;
	const userOnBeforeConnect = actorConfig.onBeforeConnect;

	return actor({
		...actorConfig,
		options: {
			actionTimeout: DEFAULT_ACTION_TIMEOUT_MS,
			sleepGracePeriod: DEFAULT_SLEEP_GRACE_PERIOD_MS,
			...actorConfig.options,
			enableActorRuntimeSocket: true,
		},
		db: db({ onMigrate: migrateAgentOsActorTables }),
		events: { ...(actorConfig.events ?? {}), ...builtInEvents },
		actions: { ...(actorConfig.actions ?? {}), ...actions },
		onBeforeConnect: async (
			c: Parameters<NonNullable<typeof userOnBeforeConnect>>[0],
			params: Parameters<NonNullable<typeof userOnBeforeConnect>>[1],
		) => {
			if (
				c.request &&
				matchPreviewPath(new URL(c.request.url).pathname) !== null
			) {
				return;
			}
			await userOnBeforeConnect?.(c, params);
		},
		onWake: async (c: AnyContext) => {
			try {
				await userOnWake?.(c);
			} catch (error) {
				await disposeVm(c as AnyContext, "error");
				throw error;
			}
		},
		onSleep: async (c: AnyContext) => {
			try {
				await userOnSleep?.(c);
			} finally {
				await disposeVm(c as AnyContext, "sleep");
			}
		},
		onDestroy: async (c: AnyContext) => {
			try {
				await userOnDestroy?.(c);
			} finally {
				await disposeVm(c as AnyContext, "destroy");
			}
		},
		onRequest: async (c: AnyContext, request: Request) => {
			const url = new URL(request.url);
			const match = matchPreviewPath(url.pathname);
			if (!match) {
				const response = await userOnRequest?.(c as never, request);
				return response ?? new Response("Not Found", { status: 404 });
			}
			if (request.method === "OPTIONS")
				return new Response(null, {
					status: 204,
					headers: {
						"access-control-allow-origin": "*",
						"access-control-allow-methods":
							"GET, POST, PUT, PATCH, DELETE, OPTIONS",
						"access-control-allow-headers": "*",
					},
				});
			const now = Date.now();
			await c.db.execute(
				"DELETE FROM agentos_actor_preview_tokens WHERE expires_at_ms <= ?",
				now,
			);
			const rows = await c.db.execute<{ port: number }>(
				"SELECT port FROM agentos_actor_preview_tokens WHERE token = ? AND expires_at_ms > ?",
				match[1],
				now,
			);
			if (!rows[0])
				return new Response("Preview URL expired or invalid", { status: 403 });
			const target = new URL(request.url);
			target.pathname = match[2] ?? "/";
			const vm = await ensureVm(c as AnyContext, agentOsOptions);
			const requestHeaders: Record<string, string> = {};
			request.headers.forEach((value, key) => {
				requestHeaders[key] = value;
			});
			const response = await vm.httpRequest({
				port: rows[0].port,
				path: `${target.pathname}${target.search}`,
				method: request.method,
				headers: requestHeaders,
				...(request.method === "GET" || request.method === "HEAD"
					? {}
					: { body: new Uint8Array(await request.arrayBuffer()) }),
			});
			const headers = new Headers(response.headers);
			headers.set("access-control-allow-origin", "*");
			return new Response(Buffer.from(response.body), {
				status: response.status,
				statusText: response.statusText,
				headers,
			});
		},
	} as any) as ActorDefinition<
		TState,
		TConnParams,
		TConnState,
		TVars,
		TInput,
		ActorDb,
		TEvents & BuiltInEvents,
		TQueues,
		TUserActions & AgentOsActions
	>;
}
