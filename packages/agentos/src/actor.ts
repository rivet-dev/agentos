import crypto from "node:crypto";
import { posix as posixPath } from "node:path";
import {
	AgentOs,
	type AgentOsOptions,
	type CronJobInfo,
	type JsonRpcNotification,
	type PermissionReply,
	type PermissionRequest,
} from "@rivet-dev/agentos-core";
import {
	type Actions,
	type ActorConfigInput,
	type ActorContext,
	type ActorDefinition,
	actor,
	event,
	type Type,
} from "rivetkit";
import { type DatabaseProvider, db, type RawAccess } from "rivetkit/db";
import type {
	AgentCrashedPayload,
	AgentOsEvents,
	CronEventPayload,
	PermissionRequestPayload,
	PersistedSessionEvent,
	PersistedSessionRecord,
	ProcessExitPayload,
	ProcessOutputPayload,
	SerializableCronJobInfo,
	SerializableCronJobOptions,
	SessionEventPayload,
	ShellDataPayload,
	ShellExitPayload,
	VmBootedPayload,
	VmShutdownPayload,
} from "./types.js";

const DEFAULT_ACTION_TIMEOUT_MS = 15 * 60_000;
const DEFAULT_SLEEP_GRACE_PERIOD_MS = 15 * 60_000;
const DEFAULT_PREVIEW_TTL_SECONDS = 3_600;
const MAX_PREVIEW_TTL_SECONDS = 86_400;
const ACTOR_SQLITE_CHUNK_SIZE = 512 * 1024;
const ACTOR_SQLITE_INLINE_THRESHOLD = 64 * 1024;
const ROOT_NAMESPACE = "agentos-root";

type BuiltInEvents = {
	[K in keyof AgentOsEvents]: Type<AgentOsEvents[K]>;
};

const builtInEvents: BuiltInEvents = {
	sessionEvent: event<SessionEventPayload>(),
	permissionRequest: event<PermissionRequestPayload>(),
	vmBooted: event<VmBootedPayload>(),
	vmShutdown: event<VmShutdownPayload>(),
	processOutput: event<ProcessOutputPayload>(),
	processExit: event<ProcessExitPayload>(),
	shellData: event<ShellDataPayload>(),
	cronEvent: event<CronEventPayload>(),
	agentCrashed: event<AgentCrashedPayload>(),
	shellStderr: event<ShellDataPayload>(),
	shellExit: event<ShellExitPayload>(),
};
type ActorDb = DatabaseProvider<RawAccess>;
type EventSchemaConfig = Record<string, any>;
type QueueSchemaConfig = Record<string, any>;
type AnyContext = ActorContext<any, any, any, any, any, ActorDb, any, any>;

interface RuntimeState {
	vm: Promise<AgentOs> | null;
	sessions: Set<string>;
	sessionHolds: Map<string, () => void>;
}

const runtimes = new Map<string, RuntimeState>();

function runtimeFor(c: AnyContext): RuntimeState {
	let runtime = runtimes.get(c.actorId);
	if (!runtime) {
		runtime = { vm: null, sessions: new Set(), sessionHolds: new Map() };
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
		const vm = await AgentOs.create({
			...options,
			onAgentExit: (event) => {
				c.broadcast("agentCrashed", { sessionId: event.sessionId, event });
				options?.onAgentExit?.(event);
			},
			rootFilesystem: {
				type: "native",
				plugin: {
					id: "chunked_actor_sqlite",
					config: {
						path,
						token,
						namespace: ROOT_NAMESPACE,
						chunkSize: ACTOR_SQLITE_CHUNK_SIZE,
						inlineThreshold: ACTOR_SQLITE_INLINE_THRESHOLD,
					},
				},
			},
		});
		vm.onCronEvent((cronEvent) => {
			c.broadcast("cronEvent", {
				event: {
					...cronEvent,
					time: cronEvent.time.getTime(),
					...(cronEvent.type === "cron:error"
						? { error: cronEvent.error.message }
						: {}),
				},
			});
		});
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
	for (const release of runtime.sessionHolds.values()) release();
	if (vm) await (await vm).dispose();
	c.broadcast("vmShutdown", { reason });
}

async function migrateAgentOsTables(database: RawAccess): Promise<void> {
	await database.execute(`
		CREATE TABLE IF NOT EXISTS agent_os_preview_tokens (
			token TEXT PRIMARY KEY,
			port INTEGER NOT NULL,
			created_at INTEGER NOT NULL,
			expires_at INTEGER NOT NULL
		);
		CREATE INDEX IF NOT EXISTS idx_agent_os_preview_tokens_expires_at
			ON agent_os_preview_tokens(expires_at);
		CREATE TABLE IF NOT EXISTS agent_os_sessions (
			session_id TEXT PRIMARY KEY,
			agent_type TEXT NOT NULL,
			created_at INTEGER NOT NULL
		);
		CREATE TABLE IF NOT EXISTS agent_os_session_events (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			session_id TEXT NOT NULL,
			seq INTEGER NOT NULL,
			event TEXT NOT NULL,
			created_at INTEGER NOT NULL,
			FOREIGN KEY (session_id) REFERENCES agent_os_sessions(session_id) ON DELETE CASCADE
		);
		CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_os_session_events_session_seq
			ON agent_os_session_events(session_id, seq);
	`);
}

function serializeCronJob(job: CronJobInfo): SerializableCronJobInfo {
	if (job.action.type === "callback") {
		throw new TypeError("callback cron actions are not serializable");
	}
	return {
		id: job.id,
		schedule: job.schedule,
		action:
			job.action.type === "session"
				? {
						type: "session",
						agentType: job.action.agentType,
						prompt: job.action.prompt,
						cwd: job.action.options?.cwd,
					}
				: {
						type: "exec",
						command: job.action.command,
						args: job.action.args,
					},
		overlap: job.overlap,
		lastRun: job.lastRun?.toISOString(),
		nextRun: job.nextRun?.toISOString(),
		runCount: job.runCount,
		running: job.running,
	};
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
	body: Uint8Array;
}

interface AgentOsEventHooks {
	onSessionEvent?: (
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
}

export function createAgentOsActions(
	options?: AgentOsOptions,
	hooks: AgentOsEventHooks = {},
	preview: AgentOsActorExtras["preview"] = {},
) {
	const defaultPreviewTtlSeconds =
		preview.defaultExpiresInSeconds ?? DEFAULT_PREVIEW_TTL_SECONDS;
	const maxPreviewTtlSeconds =
		preview.maxExpiresInSeconds ?? MAX_PREVIEW_TTL_SECONDS;
	if (
		!Number.isFinite(defaultPreviewTtlSeconds) ||
		defaultPreviewTtlSeconds <= 0 ||
		!Number.isFinite(maxPreviewTtlSeconds) ||
		maxPreviewTtlSeconds <= 0 ||
		defaultPreviewTtlSeconds > maxPreviewTtlSeconds
	) {
		throw new RangeError(
			"preview expiry bounds must be positive and the default cannot exceed the maximum",
		);
	}
	return {
		readFile: async (c: AnyContext, ...args: Parameters<AgentOs["readFile"]>) =>
			(await ensureVm(c, options)).readFile(...args),
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
		readdirEntries: async (c: AnyContext, path: string) => {
			const vm = await ensureVm(c, options);
			const names = await vm.readdir(path);
			return Promise.all(
				names.map(async (name) => {
					const stat = await vm.stat(posixPath.join(path, name));
					return {
						name,
						isDirectory: stat.isDirectory,
						isSymbolicLink: stat.isSymbolicLink,
					};
				}),
			);
		},
		readdirRecursive: async (
			c: AnyContext,
			...args: Parameters<AgentOs["readdirRecursive"]>
		) => (await ensureVm(c, options)).readdirRecursive(...args),
		exists: async (c: AnyContext, ...args: Parameters<AgentOs["exists"]>) =>
			(await ensureVm(c, options)).exists(...args),
		move: async (c: AnyContext, ...args: Parameters<AgentOs["move"]>) =>
			(await ensureVm(c, options)).move(...args),
		deleteFile: async (c: AnyContext, ...args: Parameters<AgentOs["delete"]>) =>
			(await ensureVm(c, options)).delete(...args),
		exec: async (c: AnyContext, ...args: Parameters<AgentOs["exec"]>) =>
			(await ensureVm(c, options)).exec(...args),
		spawn: async (
			c: AnyContext,
			command: string,
			args: string[],
			spawnOptions?: Omit<
				NonNullable<Parameters<AgentOs["spawn"]>[2]>,
				"onStdout" | "onStderr"
			>,
		) => {
			const vm = await ensureVm(c, options);
			const process = vm.spawn(command, args, {
				...spawnOptions,
				onStdout: (data) =>
					c.broadcast("processOutput", {
						pid: process.pid,
						stream: "stdout",
						data,
					}),
				onStderr: (data) =>
					c.broadcast("processOutput", {
						pid: process.pid,
						stream: "stderr",
						data,
					}),
			});
			void c
				.keepAwake(
					vm.waitProcess(process.pid).then((exitCode) => {
						c.broadcast("processExit", { pid: process.pid, exitCode });
						return exitCode;
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
			shellOptions?: Omit<
				NonNullable<Parameters<AgentOs["openShell"]>[0]>,
				"onStderr"
			>,
		) => {
			const vm = await ensureVm(c, options);
			const shell = vm.openShell({
				...shellOptions,
				onStderr: (data) =>
					c.broadcast("shellStderr", { shellId: shell.shellId, data }),
			});
			vm.onShellData(shell.shellId, (data) =>
				c.broadcast("shellData", { shellId: shell.shellId, data }),
			);
			void c
				.keepAwake(
					vm.waitShell(shell.shellId).then((exitCode) => {
						c.broadcast("shellExit", { shellId: shell.shellId, exitCode });
						return exitCode;
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
			(await ensureVm(c, options)).listCronJobs().map(serializeCronJob),
		cancelCronJob: async (
			c: AnyContext,
			...args: Parameters<AgentOs["cancelCronJob"]>
		) => (await ensureVm(c, options)).cancelCronJob(...args),
		listAgents: async (c: AnyContext) =>
			(await ensureVm(c, options)).listAgents(),
		createSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["createSession"]>
		) => {
			const vm = await ensureVm(c, options);
			const { sessionId } = await vm.createSession(...args);
			const runtime = runtimeFor(c);
			runtime.sessions.add(sessionId);
			const sessionHold = new Promise<void>((resolve) => {
				runtime.sessionHolds.set(sessionId, resolve);
			});
			void c.keepAwake(sessionHold).catch((error) =>
				c.log.error({
					msg: "agent-os session hold failed",
					sessionId,
					error,
				}),
			);
			await c.db.execute(
				"INSERT OR REPLACE INTO agent_os_sessions (session_id, agent_type, created_at) VALUES (?, ?, ?)",
				sessionId,
				String(args[0]),
				Date.now(),
			);
			let seq = 0;
			vm.onSessionEvent(sessionId, (notification: JsonRpcNotification) => {
				const serialized = JSON.parse(
					JSON.stringify(notification),
				) as JsonRpcNotification;
				seq += 1;
				const task = c.db
					.execute(
						"INSERT INTO agent_os_session_events (session_id, seq, event, created_at) VALUES (?, ?, ?, ?)",
						sessionId,
						seq,
						JSON.stringify(serialized),
						Date.now(),
					)
					.then(async () => {
						c.broadcast("sessionEvent", { sessionId, event: serialized });
						await hooks.onSessionEvent?.(sessionId, serialized);
					});
				c.waitUntil(
					task.catch((error) =>
						c.log.error({
							msg: "agent-os session event persistence failed",
							sessionId,
							error,
						}),
					),
				);
			});
			vm.onPermissionRequest(sessionId, (request: PermissionRequest) => {
				c.broadcast("permissionRequest", { sessionId, request });
				if (hooks.onPermissionRequest) {
					c.waitUntil(
						Promise.resolve()
							.then(() => hooks.onPermissionRequest?.(sessionId, request))
							.catch((error) =>
								c.log.error({
									msg: "agent-os permission hook failed",
									sessionId,
									error,
								}),
							),
					);
				}
			});
			return sessionId;
		},
		sendPrompt: async (c: AnyContext, sessionId: string, text: string) => {
			const promise = (await ensureVm(c, options)).prompt(sessionId, text);
			return c.keepAwake(promise);
		},
		closeSession: async (c: AnyContext, sessionId: string) => {
			const vm = await ensureVm(c, options);
			vm.closeSession(sessionId);
			const runtime = runtimeFor(c);
			runtime.sessions.delete(sessionId);
			runtime.sessionHolds.get(sessionId)?.();
			runtime.sessionHolds.delete(sessionId);
		},
		respondPermission: async (
			c: AnyContext,
			sessionId: string,
			permissionId: string,
			reply: PermissionReply,
		) =>
			(await ensureVm(c, options)).respondPermission(
				sessionId,
				permissionId,
				reply,
			),
		listPersistedSessions: async (
			c: AnyContext,
		): Promise<PersistedSessionRecord[]> => {
			const rows = await c.db.execute<{
				session_id: string;
				agent_type: string;
				created_at: number;
			}>(
				"SELECT session_id, agent_type, created_at FROM agent_os_sessions ORDER BY created_at DESC",
			);
			return rows.map((row) => ({
				sessionId: row.session_id,
				agentType: row.agent_type,
				createdAt: row.created_at,
				status: runtimeFor(c).sessions.has(row.session_id) ? "running" : "idle",
			}));
		},
		getSessionEvents: async (
			c: AnyContext,
			sessionId: string,
		): Promise<PersistedSessionEvent[]> => {
			const rows = await c.db.execute<{
				seq: number;
				event: string;
				created_at: number;
			}>(
				"SELECT seq, event, created_at FROM agent_os_session_events WHERE session_id = ? ORDER BY seq",
				sessionId,
			);
			return rows.map((row) => ({
				sessionId,
				seq: row.seq,
				event: JSON.parse(row.event),
				createdAt: row.created_at,
			}));
		},
		createSignedPreviewUrl: async (
			c: AnyContext,
			port: number,
			ttlSeconds = defaultPreviewTtlSeconds,
		) => {
			if (!Number.isInteger(port) || port < 1 || port > 65_535)
				throw new RangeError("port must be an integer between 1 and 65535");
			if (
				!Number.isFinite(ttlSeconds) ||
				ttlSeconds <= 0 ||
				ttlSeconds > maxPreviewTtlSeconds
			)
				throw new RangeError(
					`ttlSeconds must be between 0 and ${maxPreviewTtlSeconds}`,
				);
			const token = crypto.randomBytes(24).toString("hex");
			const createdAt = Date.now();
			const expiresAt = createdAt + ttlSeconds * 1_000;
			await c.db.execute(
				"INSERT INTO agent_os_preview_tokens (token, port, created_at, expires_at) VALUES (?, ?, ?, ?)",
				token,
				port,
				createdAt,
				expiresAt,
			);
			return { path: `/fetch/${token}`, token, port, expiresAt };
		},
		expireSignedPreviewUrl: async (c: AnyContext, token: string) => {
			await c.db.execute(
				"DELETE FROM agent_os_preview_tokens WHERE token = ?",
				token,
			);
		},
		listMounts: async () => options?.mounts ?? [],
		listSoftware: async (c: AnyContext) =>
			(await ensureVm(c, options)).providedCommands(),
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

export interface AgentOsActorExtras extends AgentOsOptions, AgentOsEventHooks {
	preview?: {
		defaultExpiresInSeconds?: number;
		maxExpiresInSeconds?: number;
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
	AgentOsActorExtras;

const agentOsOptionKeys = [
	"software",
	"defaultSoftware",
	"loopbackExemptPorts",
	"allowedNodeBuiltins",
	"highResolutionTime",
	"rootFilesystem",
	"mounts",
	"additionalInstructions",
	"scheduleDriver",
	"toolKits",
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
		actorConfig.onSessionEvent as AgentOsEventHooks["onSessionEvent"];
	const onPermissionRequest =
		actorConfig.onPermissionRequest as AgentOsEventHooks["onPermissionRequest"];
	const preview = actorConfig.preview as AgentOsActorExtras["preview"];
	delete actorConfig.onSessionEvent;
	delete actorConfig.onPermissionRequest;
	delete actorConfig.preview;
	return {
		actorConfig,
		agentOsOptions,
		hooks: { onSessionEvent, onPermissionRequest },
		preview,
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
	const { agentOsOptions, hooks, preview } = split;
	if (agentOsOptions.rootFilesystem) {
		throw new Error(
			"agentOS() owns rootFilesystem so it can persist directly through the actor SQLite UDS; use mounts for additional filesystems",
		);
	}
	const actions = createAgentOsActions(agentOsOptions, hooks, preview);
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
		db: db({ onMigrate: migrateAgentOsTables }),
		events: { ...(actorConfig.events ?? {}), ...builtInEvents },
		actions: { ...(actorConfig.actions ?? {}), ...actions },
		onBeforeConnect: async (
			c: Parameters<NonNullable<typeof userOnBeforeConnect>>[0],
			params: Parameters<NonNullable<typeof userOnBeforeConnect>>[1],
		) => {
			if (c.request && new URL(c.request.url).pathname.startsWith("/fetch/")) {
				return;
			}
			await userOnBeforeConnect?.(c, params);
		},
		onWake: async (c: AnyContext) => {
			try {
				await ensureVm(c as AnyContext, agentOsOptions);
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
			const match = url.pathname.match(/^\/fetch\/([a-f0-9]{48})(\/.*)?$/);
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
			const rows = await c.db.execute<{ port: number }>(
				"SELECT port FROM agent_os_preview_tokens WHERE token = ? AND expires_at > ?",
				match[1],
				Date.now(),
			);
			if (!rows[0])
				return new Response("Preview URL expired or invalid", { status: 403 });
			const target = new URL(request.url);
			target.pathname = match[2] ?? "/";
			const vm = await ensureVm(c as AnyContext, agentOsOptions);
			const response = await vm.fetch(
				rows[0].port,
				new Request(target, request),
			);
			const headers = new Headers(response.headers);
			headers.set("access-control-allow-origin", "*");
			return new Response(response.body, {
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
