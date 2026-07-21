import crypto from "node:crypto";
import {
	type AgentExitEvent,
	AgentOs,
	type AgentOsOptions,
	type CodeExecutionResult,
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
import type {
	ActorData,
	ActorInlineExecutionOptions,
	ActorJavaScriptExecutionOptions,
	ActorLanguageExecutionOptions,
	ActorNpmPackageInstallOptions,
	ActorNpmProjectInstallOptions,
	ActorPythonInstallOptions,
	ActorTypeScriptCheckOptions,
	ActorTypeScriptExecutionOptions,
	ActorTypeScriptFileExecutionOptions,
	AgentOsEvents,
	ExecutionCompletedPayload,
	ExecutionOutputPayload,
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
	executionOutput: event<ExecutionOutputPayload>(),
	executionCompleted: event<ExecutionCompletedPayload>(),
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
	executionUnsubscribers: readonly (() => void)[];
}

const runtimes = new Map<string, RuntimeState>();

function runtimeFor(c: AnyContext): RuntimeState {
	let runtime = runtimes.get(c.actorId);
	if (!runtime) {
		runtime = {
			vm: null,
			subscribedSessions: new Map(),
			executionUnsubscribers: [],
		};
		runtimes.set(c.actorId, runtime);
	}
	return runtime;
}

async function ensureVm(
	c: AnyContext,
	options?: AgentOsOptions,
): Promise<AgentOs> {
	const runtime = runtimeFor(c);
	if (runtime.vm !== null) return runtime.vm;

	const startedAt = Date.now();
	runtime.vm = (async () => {
		const actorUds = (
			c as AnyContext & {
				actorUds(): Promise<{ path: string; token: string }>;
			}
		).actorUds;
		if (typeof actorUds !== "function") {
			throw new Error(
				"AgentOS actors require a RivetKit runtime with experimental actor UDS support",
			);
		}
		const { path, token } = await actorUds.call(c);
		const mountRows = await c.db.execute<{ descriptor_json: string }>(
			"SELECT descriptor_json FROM agentos_actor_dynamic_mounts ORDER BY path",
		);
		const softwareRows = await c.db.execute<{ descriptor_json: string }>(
			"SELECT descriptor_json FROM agentos_actor_linked_software ORDER BY path",
		);
		const vm = await AgentOs.create({
			...options,
			database: { type: "actor_uds", path, token },
			onAgentExit: (event) => {
				c.log.error({
					msg: "agent-os agent adapter exited unexpectedly",
					...event,
				});
				c.broadcast("agentExit", event);
				try {
					options?.onAgentExit?.(event);
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
						uid: options?.user?.euid ?? options?.user?.uid ?? 1000,
						gid: options?.user?.egid ?? options?.user?.gid ?? 1000,
					},
				},
			},
		});
		try {
			for (const row of mountRows) {
				await vm.filesystem.mount(
					JSON.parse(row.descriptor_json) as DynamicMountDescriptor,
				);
			}
			for (const row of softwareRows) {
				await vm.software.link(
					JSON.parse(row.descriptor_json) as PackageDescriptor,
				);
			}
		} catch (error) {
			await vm.dispose();
			throw error;
		}
		vm.onCronEvent((cronEvent) => c.broadcast("cronEvent", cronEvent));
		runtime.executionUnsubscribers = [
			vm.onExecutionOutput("*", (event) =>
				c.broadcast("executionOutput", {
					...event,
					chunk: {
						encoding: "base64",
						data: Buffer.from(event.chunk).toString("base64"),
					},
				}),
			),
			vm.onExecutionCompleted("*", (event) =>
				c.broadcast("executionCompleted", event),
			),
		];
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
	for (const unsubscribe of runtime.executionUnsubscribers) unsubscribe();
	if (vm) await (await vm).dispose();
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
	await database.execute(`
		CREATE TABLE IF NOT EXISTS agentos_actor_schema_version (
			singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
			schema_version INTEGER NOT NULL CHECK (schema_version BETWEEN 0 AND ${MAX_SQLITE_SAFE_INTEGER})
		) STRICT;
	`);
	const rows = await database.execute<{ schema_version: unknown }>(
		"SELECT schema_version FROM agentos_actor_schema_version WHERE singleton = 1",
	);
	if (rows.length > 1) {
		throw new Error(
			`invalid AgentOS actor SQLite schema version table: expected at most one row, received ${rows.length}`,
		);
	}
	const rawCurrent = rows[0]?.schema_version;
	const current = rows.length === 0 ? 0 : rawCurrent;
	if (
		typeof current !== "number" ||
		!Number.isSafeInteger(current) ||
		current < 0
	) {
		throw new Error(
			`invalid AgentOS actor SQLite schema version ${String(rawCurrent)}`,
		);
	}
	if (current > ACTOR_SQLITE_MIGRATIONS.length) {
		throw new Error(
			`AgentOS actor SQLite schema version ${current} is newer than supported version ${ACTOR_SQLITE_MIGRATIONS.length}`,
		);
	}
	for (const migration of ACTOR_SQLITE_MIGRATIONS.slice(current)) {
		await database.execute(migration.sql);
		await database.execute(
			`INSERT INTO agentos_actor_schema_version (singleton, schema_version)
			 VALUES (1, ?)
			 ON CONFLICT(singleton) DO UPDATE SET schema_version = excluded.schema_version`,
			migration.version,
		);
	}
}

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
				c.waitUntil(
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

function decodeActorData(value: ActorData): string | Uint8Array;
function decodeActorData(value: unknown): string | Uint8Array | undefined;
function decodeActorData(value: unknown): string | Uint8Array | undefined {
	if (value === undefined) return undefined;
	if (
		typeof value !== "object" ||
		value === null ||
		!("encoding" in value) ||
		!("data" in value)
	) {
		throw new UserError("stdin must be tagged UTF-8 or base64 actor data", {
			code: "agentos_execution_invalid_actor_data",
		});
	}
	const input = value as { encoding: unknown; data: unknown };
	if (typeof input.data !== "string") {
		throw new UserError("actor data must contain a string", {
			code: "agentos_execution_invalid_actor_data",
		});
	}
	if (input.encoding === "utf8") return input.data;
	if (input.encoding === "base64")
		return new Uint8Array(Buffer.from(input.data, "base64"));
	throw new UserError("actor data encoding must be utf8 or base64", {
		code: "agentos_execution_invalid_actor_data",
	});
}

function actorExecutionArgs(args: readonly unknown[]): unknown[] {
	return args.map((arg) => {
		if (typeof arg !== "object" || arg === null || Array.isArray(arg))
			return arg;
		const options = arg as Record<string, unknown>;
		for (const field of ["signal", "onStdout", "onStderr"] as const) {
			if (options[field] !== undefined) {
				throw new UserError(`${field} is not available on actor actions`, {
					code: "agentos_execution_non_serializable_option",
				});
			}
		}
		const normalized = { ...options };
		if ("stdin" in normalized)
			normalized.stdin = decodeActorData(normalized.stdin);
		return normalized;
	});
}

async function runActorExecution<
	T extends { executionId: string; detached: boolean },
>(
	c: AnyContext,
	operation: (vm: AgentOs) => Promise<T>,
	options?: AgentOsOptions,
): Promise<T> {
	const vm = await ensureVm(c, options);
	const result = await c.keepAwake(operation(vm));
	if (result.detached) {
		void c.keepAwake(vm.executions.wait(result.executionId)).catch((error) =>
			c.log.error({
				msg: "agent-os detached execution wait failed",
				executionId: result.executionId,
				error,
			}),
		);
	}
	return result;
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
	const flat = {
		readFile: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["readFile"]>
		) => {
			try {
				return await (await ensureVm(c, options)).filesystem.readFile(...args);
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
			...args: Parameters<AgentOs["filesystem"]["writeFile"]>
		) => (await ensureVm(c, options)).filesystem.writeFile(...args),
		readFiles: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["readFiles"]>
		) => (await ensureVm(c, options)).filesystem.readFiles(...args),
		writeFiles: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["writeFiles"]>
		) => (await ensureVm(c, options)).filesystem.writeFiles(...args),
		stat: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["stat"]>
		) => (await ensureVm(c, options)).filesystem.stat(...args),
		mkdir: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["mkdir"]>
		) => (await ensureVm(c, options)).filesystem.mkdir(...args),
		readdir: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["readdir"]>
		) => (await ensureVm(c, options)).filesystem.readdir(...args),
		readdirEntries: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["readdirEntries"]>
		) => (await ensureVm(c, options)).filesystem.readdirEntries(...args),
		readdirRecursive: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["readdirRecursive"]>
		) => (await ensureVm(c, options)).filesystem.readdirRecursive(...args),
		exists: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["exists"]>
		) => (await ensureVm(c, options)).filesystem.exists(...args),
		move: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["move"]>
		) => (await ensureVm(c, options)).filesystem.move(...args),
		remove: async (
			c: AnyContext,
			...args: Parameters<AgentOs["filesystem"]["remove"]>
		) => (await ensureVm(c, options)).filesystem.remove(...args),
		exec: async (
			c: AnyContext,
			command: string,
			actorOptions?: ActorLanguageExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.process.exec(
						command,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		execArgv: async (
			c: AnyContext,
			command: string,
			args: readonly string[] = [],
			actorOptions?: Omit<ActorLanguageExecutionOptions, "args">,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.process.execFile(
						command,
						args,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		spawn: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["spawn"]>
		) => {
			const vm = await ensureVm(c, options);
			const process = vm.process.spawn(...args);
			const unsubscribeOutput = vm.onProcessOutput(process.pid, (event) =>
				c.broadcast("processOutput", event),
			);
			const unsubscribeExit = vm.onProcessExit(process.pid, (event) =>
				c.broadcast("processExit", event),
			);
			void c
				.keepAwake(
					vm.process.wait(process.pid).finally(() => {
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
		executeJavaScript: async (
			c: AnyContext,
			source: string,
			actorOptions?: ActorJavaScriptExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.execute(
						source,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		evaluateJavaScript: async (
			c: AnyContext,
			expression: string,
			actorOptions?: Omit<ActorJavaScriptExecutionOptions, "detached">,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.evaluate(
						expression,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executeJavaScriptFile: async (
			c: AnyContext,
			path: string,
			actorOptions?: ActorLanguageExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.executeFile(
						path,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executeTypeScript: async (
			c: AnyContext,
			source: string,
			actorOptions?: ActorTypeScriptExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.typescript.execute(
						source,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		evaluateTypeScript: async (
			c: AnyContext,
			expression: string,
			actorOptions?: Omit<ActorTypeScriptExecutionOptions, "detached">,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.typescript.evaluate(
						expression,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executeTypeScriptFile: async (
			c: AnyContext,
			path: string,
			actorOptions?: ActorTypeScriptFileExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.typescript.executeFile(
						path,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		checkTypeScript: async (
			c: AnyContext,
			source: string,
			actorOptions?: ActorTypeScriptCheckOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.typescript.check(
						source,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		checkTypeScriptProject: async (
			c: AnyContext,
			actorOptions?: Omit<
				ActorTypeScriptCheckOptions,
				"filePath" | "compilerOptions"
			>,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.typescript.checkProject(
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		installNpmPackages: async (
			c: AnyContext,
			...args:
				| [options?: ActorNpmProjectInstallOptions]
				| [packages: string | string[], options?: ActorNpmPackageInstallOptions]
		) =>
			runActorExecution(
				c,
				(vm) =>
					(
						vm.javascript.npm.install as (
							...callArgs: unknown[]
						) => Promise<CodeExecutionResult>
					)(...actorExecutionArgs(args)),
				options,
			),
		executeNpmScript: async (
			c: AnyContext,
			script: string,
			actorOptions?: ActorLanguageExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.npm.runScript(
						script,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executeNpmPackage: async (
			c: AnyContext,
			packageSpec: string,
			actorOptions?: ActorLanguageExecutionOptions & { binary?: string },
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.javascript.npm.runPackage(
						packageSpec,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executePython: async (
			c: AnyContext,
			source: string,
			actorOptions?: ActorInlineExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.python.execute(
						source,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		evaluatePython: async (
			c: AnyContext,
			expression: string,
			actorOptions?: Omit<ActorInlineExecutionOptions, "detached">,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.python.evaluate(
						expression,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executePythonFile: async (
			c: AnyContext,
			path: string,
			actorOptions?: ActorLanguageExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.python.executeFile(
						path,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		executePythonModule: async (
			c: AnyContext,
			module: string,
			actorOptions?: ActorLanguageExecutionOptions,
		) =>
			runActorExecution(
				c,
				(vm) =>
					vm.python.executeModule(
						module,
						...(actorExecutionArgs([actorOptions]) as [never]),
					),
				options,
			),
		installPythonPackages: async (
			c: AnyContext,
			...args:
				| [options?: ActorPythonInstallOptions]
				| [packages: string | string[], options?: ActorPythonInstallOptions]
		) =>
			runActorExecution(
				c,
				(vm) =>
					(
						vm.python.install as (
							...callArgs: unknown[]
						) => Promise<CodeExecutionResult>
					)(...actorExecutionArgs(args)),
				options,
			),
		getExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["get"]>
		) => (await ensureVm(c, options)).executions.get(...args),
		listExecutions: async (c: AnyContext) =>
			(await ensureVm(c, options)).executions.list(),
		waitExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["wait"]>
		) => (await ensureVm(c, options)).executions.wait(...args),
		cancelExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["cancel"]>
		) => (await ensureVm(c, options)).executions.cancel(...args),
		signalExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["signal"]>
		) => (await ensureVm(c, options)).executions.signal(...args),
		resetExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["reset"]>
		) => (await ensureVm(c, options)).executions.reset(...args),
		deleteExecution: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["delete"]>
		) => (await ensureVm(c, options)).executions.delete(...args),
		writeExecutionStdin: async (
			c: AnyContext,
			executionId: string,
			data: ActorData,
		) =>
			(await ensureVm(c, options)).executions.writeStdin(
				executionId,
				decodeActorData(data),
			),
		closeExecutionStdin: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["closeStdin"]>
		) => (await ensureVm(c, options)).executions.closeStdin(...args),
		resizeExecutionPty: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["resizePty"]>
		) => (await ensureVm(c, options)).executions.resizePty(...args),
		readExecutionOutput: async (
			c: AnyContext,
			...args: Parameters<AgentOs["executions"]["readOutput"]>
		) => {
			const page = await (await ensureVm(c, options)).executions.readOutput(
				...args,
			);
			return {
				...page,
				events: page.events.map((event) => ({
					...event,
					chunk: {
						encoding: "base64" as const,
						data: Buffer.from(event.chunk).toString("base64"),
					},
				})),
			};
		},
		waitProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["wait"]>
		) => (await ensureVm(c, options)).process.wait(...args),
		killProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["kill"]>
		) => (await ensureVm(c, options)).process.kill(...args),
		stopProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["stop"]>
		) => (await ensureVm(c, options)).process.stop(...args),
		listProcesses: async (c: AnyContext) =>
			(await ensureVm(c, options)).process.list(),
		allProcesses: async (c: AnyContext) =>
			(await ensureVm(c, options)).process.listAll(),
		processTree: async (c: AnyContext) =>
			(await ensureVm(c, options)).process.tree(),
		getProcess: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["get"]>
		) => (await ensureVm(c, options)).process.get(...args),
		writeProcessStdin: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["writeStdin"]>
		) => (await ensureVm(c, options)).process.writeStdin(...args),
		closeProcessStdin: async (
			c: AnyContext,
			...args: Parameters<AgentOs["process"]["closeStdin"]>
		) => (await ensureVm(c, options)).process.closeStdin(...args),
		openShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["terminal"]["open"]>
		) => {
			const vm = await ensureVm(c, options);
			const shell = vm.terminal.open(...args);
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
					vm.terminal.wait(shell.shellId).finally(() => {
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
			...args: Parameters<AgentOs["terminal"]["write"]>
		) => (await ensureVm(c, options)).terminal.write(...args),
		resizeShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["terminal"]["resize"]>
		) => (await ensureVm(c, options)).terminal.resize(...args),
		closeShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["terminal"]["close"]>
		) => (await ensureVm(c, options)).terminal.close(...args),
		waitShell: async (
			c: AnyContext,
			...args: Parameters<AgentOs["terminal"]["wait"]>
		) => (await ensureVm(c, options)).terminal.wait(...args),
		httpRequest: async (
			c: AnyContext,
			...args: Parameters<AgentOs["network"]["httpRequest"]>
		) => (await ensureVm(c, options)).network.httpRequest(...args),
		scheduleCron: async (
			c: AnyContext,
			cronOptions: SerializableCronJobOptions,
		) => {
			const job = (await ensureVm(c, options)).cron.schedule(
				cronOptions as Parameters<AgentOs["cron"]["schedule"]>[0],
			);
			return { id: job.id };
		},
		listCronJobs: async (c: AnyContext) =>
			(await ensureVm(c, options)).cron.list(),
		cancelCronJob: async (
			c: AnyContext,
			...args: Parameters<AgentOs["cron"]["cancel"]>
		) => (await ensureVm(c, options)).cron.cancel(...args),
		listAgents: async (c: AnyContext) =>
			(await ensureVm(c, options)).agents.list(),
		openSession: async (c: AnyContext, input: OpenSessionInput) => {
			const vm = await ensureVm(c, options);
			try {
				await vm.sessions.open(input);
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
			...args: Parameters<AgentOs["sessions"]["get"]>
		) => (await ensureVm(c, options)).sessions.get(...args),
		listSessions: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["list"]>
		) => (await ensureVm(c, options)).sessions.list(...args),
		deleteSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["delete"]>
		) => {
			const result = await (await ensureVm(c, options)).sessions.delete(
				...args,
			);
			untrackSessionEvents(c, args[0]?.sessionId ?? "main");
			return result;
		},
		unloadSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["unload"]>
		) => {
			const result = await (await ensureVm(c, options)).sessions.unload(
				...args,
			);
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
				return await c.keepAwake(vm.sessions.prompt(input));
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
			...args: Parameters<AgentOs["sessions"]["cancelPrompt"]>
		) => (await ensureVm(c, options)).sessions.cancelPrompt(...args),
		respondPermission: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["respondPermission"]>
		) => (await ensureVm(c, options)).sessions.respondPermission(...args),
		readHistory: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["readHistory"]>
		) => (await ensureVm(c, options)).sessions.readHistory(...args),
		getSessionConfig: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["getConfig"]>
		) => (await ensureVm(c, options)).sessions.getConfig(...args),
		setSessionConfigOption: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["setConfigOption"]>
		) => (await ensureVm(c, options)).sessions.setConfigOption(...args),
		getSessionCapabilities: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["getCapabilities"]>
		) => (await ensureVm(c, options)).sessions.getCapabilities(...args),
		getSessionAgentInfo: async (
			c: AnyContext,
			...args: Parameters<AgentOs["sessions"]["getAgentInfo"]>
		) => (await ensureVm(c, options)).sessions.getAgentInfo(...args),
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
			...args: Parameters<AgentOs["filesystem"]["export"]>
		) => (await ensureVm(c, options)).filesystem.export(...args),
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
			await vm.filesystem.mount(descriptor);
			try {
				await c.db.execute(
					"INSERT INTO agentos_actor_dynamic_mounts (path, descriptor_json) VALUES (?, ?) ON CONFLICT(path) DO UPDATE SET descriptor_json = excluded.descriptor_json",
					descriptor.path,
					JSON.stringify(descriptor),
				);
			} catch (error) {
				try {
					await vm.filesystem.unmount(descriptor.path);
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
			await vm.filesystem.unmount(path);
			try {
				await c.db.execute(
					"DELETE FROM agentos_actor_dynamic_mounts WHERE path = ?",
					path,
				);
			} catch (error) {
				if (rows[0]) {
					try {
						await vm.filesystem.mount(
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
			(await ensureVm(c, options)).filesystem.listMounts(),
		listSoftware: async (c: AnyContext) =>
			(await ensureVm(c, options)).software.list(),
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
				await (await ensureVm(c, options)).software.link(descriptor);
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

	const nested = {
		process: {
			exec: flat.exec,
			execFile: flat.execArgv,
			spawn: flat.spawn,
			get: flat.getProcess,
			list: flat.listProcesses,
			listAll: flat.allProcesses,
			tree: flat.processTree,
			wait: flat.waitProcess,
			stop: flat.stopProcess,
			kill: flat.killProcess,
			writeStdin: flat.writeProcessStdin,
			closeStdin: flat.closeProcessStdin,
		},
		javascript: {
			execute: flat.executeJavaScript,
			evaluate: flat.evaluateJavaScript,
			executeFile: flat.executeJavaScriptFile,
			typescript: {
				execute: flat.executeTypeScript,
				evaluate: flat.evaluateTypeScript,
				executeFile: flat.executeTypeScriptFile,
				check: flat.checkTypeScript,
				checkProject: flat.checkTypeScriptProject,
			},
			npm: {
				install: flat.installNpmPackages,
				runScript: flat.executeNpmScript,
				runPackage: flat.executeNpmPackage,
			},
		},
		python: {
			execute: flat.executePython,
			evaluate: flat.evaluatePython,
			executeFile: flat.executePythonFile,
			executeModule: flat.executePythonModule,
			install: flat.installPythonPackages,
		},
		executions: {
			get: flat.getExecution,
			list: flat.listExecutions,
			wait: flat.waitExecution,
			cancel: flat.cancelExecution,
			signal: flat.signalExecution,
			reset: flat.resetExecution,
			delete: flat.deleteExecution,
			writeStdin: flat.writeExecutionStdin,
			closeStdin: flat.closeExecutionStdin,
			resizePty: flat.resizeExecutionPty,
			readOutput: flat.readExecutionOutput,
		},
		terminal: {
			open: flat.openShell,
			write: flat.writeShell,
			resize: flat.resizeShell,
			wait: flat.waitShell,
			close: flat.closeShell,
		},
		filesystem: {
			readFile: flat.readFile,
			writeFile: flat.writeFile,
			readFiles: flat.readFiles,
			writeFiles: flat.writeFiles,
			stat: flat.stat,
			mkdir: flat.mkdir,
			readdir: flat.readdir,
			readdirEntries: flat.readdirEntries,
			readdirRecursive: flat.readdirRecursive,
			exists: flat.exists,
			move: flat.move,
			remove: flat.remove,
			export: flat.exportRootFilesystem,
			mount: flat.mountFs,
			unmount: flat.unmountFs,
			listMounts: flat.listMounts,
		},
		network: {
			httpRequest: flat.httpRequest,
		},
		software: {
			list: flat.listSoftware,
			link: flat.linkSoftware,
		},
		agents: {
			list: flat.listAgents,
		},
		sessions: {
			open: flat.openSession,
			get: flat.getSession,
			list: flat.listSessions,
			delete: flat.deleteSession,
			unload: flat.unloadSession,
			prompt: flat.prompt,
			cancelPrompt: flat.cancelPrompt,
			respondPermission: flat.respondPermission,
			readHistory: flat.readHistory,
			getConfig: flat.getSessionConfig,
			setConfigOption: flat.setSessionConfigOption,
			getCapabilities: flat.getSessionCapabilities,
			getAgentInfo: flat.getSessionAgentInfo,
		},
		cron: {
			schedule: flat.scheduleCron,
			list: flat.listCronJobs,
			cancel: flat.cancelCronJob,
		},
	};

	return {
		...nested,

		// Legacy flat actions from the pre-language-execution API. Keep these
		// as aliases to the canonical nested handlers.
		readFile: nested.filesystem.readFile,
		writeFile: nested.filesystem.writeFile,
		readFiles: nested.filesystem.readFiles,
		writeFiles: nested.filesystem.writeFiles,
		stat: nested.filesystem.stat,
		mkdir: nested.filesystem.mkdir,
		readdir: nested.filesystem.readdir,
		readdirEntries: nested.filesystem.readdirEntries,
		readdirRecursive: nested.filesystem.readdirRecursive,
		exists: nested.filesystem.exists,
		move: nested.filesystem.move,
		remove: nested.filesystem.remove,
		exportRootFilesystem: nested.filesystem.export,
		mountFs: nested.filesystem.mount,
		unmountFs: nested.filesystem.unmount,
		listMounts: nested.filesystem.listMounts,
		exec: async (c: AnyContext, ...args: Parameters<AgentOs["exec"]>) =>
			(await ensureVm(c, options)).exec(...args),
		execArgv: async (c: AnyContext, ...args: Parameters<AgentOs["execArgv"]>) =>
			(await ensureVm(c, options)).execArgv(...args),
		spawn: nested.process.spawn,
		waitProcess: nested.process.wait,
		killProcess: nested.process.kill,
		stopProcess: nested.process.stop,
		listProcesses: nested.process.list,
		allProcesses: nested.process.listAll,
		processTree: nested.process.tree,
		getProcess: nested.process.get,
		writeProcessStdin: nested.process.writeStdin,
		closeProcessStdin: nested.process.closeStdin,
		openShell: nested.terminal.open,
		writeShell: nested.terminal.write,
		resizeShell: nested.terminal.resize,
		closeShell: nested.terminal.close,
		waitShell: nested.terminal.wait,
		httpRequest: nested.network.httpRequest,
		scheduleCron: nested.cron.schedule,
		listCronJobs: nested.cron.list,
		cancelCronJob: nested.cron.cancel,
		listAgents: nested.agents.list,
		openSession: nested.sessions.open,
		getSession: nested.sessions.get,
		listSessions: nested.sessions.list,
		deleteSession: nested.sessions.delete,
		unloadSession: nested.sessions.unload,
		prompt: nested.sessions.prompt,
		cancelPrompt: nested.sessions.cancelPrompt,
		respondPermission: nested.sessions.respondPermission,
		readHistory: nested.sessions.readHistory,
		getSessionConfig: nested.sessions.getConfig,
		setSessionConfigOption: nested.sessions.setConfigOption,
		getSessionCapabilities: nested.sessions.getCapabilities,
		getSessionAgentInfo: nested.sessions.getAgentInfo,
		listSoftware: nested.software.list,
		linkSoftware: nested.software.link,

		// Preview URLs are RivetKit-specific and intentionally stay flat.
		createPreviewUrl: flat.createPreviewUrl,
		expirePreviewUrl: flat.expirePreviewUrl,
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
> = Omit<
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
	"database",
	"rootFilesystem",
	"mounts",
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
	const maxSessionSubscriptions = actorConfig.maxSessionSubscriptions as
		| number
		| undefined;
	const maxDynamicMounts = actorConfig.maxDynamicMounts as number | undefined;
	const maxLinkedSoftware = actorConfig.maxLinkedSoftware as number | undefined;
	delete actorConfig.onSessionEvent;
	delete actorConfig.preview;
	delete actorConfig.maxSessionSubscriptions;
	delete actorConfig.maxDynamicMounts;
	delete actorConfig.maxLinkedSoftware;
	return {
		actorConfig,
		agentOsOptions,
		hooks: { onSessionEvent },
		preview,
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
			const response = await vm.network.httpRequest({
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
