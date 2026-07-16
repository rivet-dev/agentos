import crypto from "node:crypto";
import { posix as posixPath } from "node:path";
import {
	AgentOs,
	type AgentOsOptions,
	type CronJobInfo,
	type JsonRpcNotification,
	type MountConfig,
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
	UserError,
} from "rivetkit";
import { type DatabaseProvider, db, type RawAccess } from "rivetkit/db";
import type {
	AgentCrashedPayload,
	AgentOsEvents,
	CronEventPayload,
	MountInfoDto,
	PermissionRequestPayload,
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
const DEFAULT_MAX_ACTIVE_PREVIEW_TOKENS = 1_024;
const ACTOR_SQLITE_CHUNK_SIZE = 512 * 1024;
const ACTOR_SQLITE_INLINE_THRESHOLD = 64 * 1024;
const ROOT_NAMESPACE = "agentos-root";
const PREVIEW_PATH_PATTERN = /^\/fetch\/([a-f0-9]{48})(\/.*)?$/;

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
	sessionHolds: Map<string, () => void>;
}

const runtimes = new Map<string, RuntimeState>();

function runtimeFor(c: AnyContext): RuntimeState {
	let runtime = runtimes.get(c.actorId);
	if (!runtime) {
		runtime = { vm: null, sessionHolds: new Map() };
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
				c.log.error({
					msg: "agent-os agent adapter exited unexpectedly",
					...event,
				});
				if (event.restart !== "restarted") {
					runtime.sessionHolds.get(event.sessionId)?.();
					runtime.sessionHolds.delete(event.sessionId);
				}
				c.broadcast("agentCrashed", { sessionId: event.sessionId, event });
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

function matchPreviewPath(pathname: string): RegExpMatchArray | null {
	return pathname.match(PREVIEW_PATH_PATTERN);
}

function serializeMount(mount: MountConfig): MountInfoDto {
	if ("plugin" in mount) {
		const config = mount.plugin.config;
		const configReadOnly =
			typeof config === "object" &&
			config !== null &&
			"readOnly" in config &&
			typeof config.readOnly === "boolean"
				? config.readOnly
				: undefined;
		return {
			path: mount.path,
			kind: mount.plugin.id,
			readOnly: mount.readOnly ?? configReadOnly ?? false,
		};
	}
	if ("filesystem" in mount) {
		return {
			path: mount.path,
			kind: "overlay",
			readOnly: mount.filesystem.mode === "read-only",
			config: { mode: mount.filesystem.mode },
		};
	}
	return {
		path: mount.path,
		kind: "custom",
		readOnly: mount.readOnly ?? false,
	};
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

export interface AgentOsEventHooks<TContext = AnyContext> {
	onSessionEvent?: (
		c: TContext,
		sessionId: string,
		event: JsonRpcNotification,
	) => void | Promise<void>;
	onPermissionRequest?: (
		c: TContext,
		sessionId: string,
		request: PermissionRequest,
	) => void | Promise<void>;
}

function trackLiveSession(
	c: AnyContext,
	vm: AgentOs,
	sessionId: string,
	hooks: AgentOsEventHooks,
): void {
	const runtime = runtimeFor(c);
	if (!runtime.sessionHolds.has(sessionId)) {
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
	}
	vm.onSessionEvent(sessionId, (notification: JsonRpcNotification) => {
		const serialized = JSON.parse(
			JSON.stringify(notification),
		) as JsonRpcNotification;
		c.broadcast("sessionEvent", { sessionId, event: serialized });
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
	});
	vm.onPermissionRequest(sessionId, (request: PermissionRequest) => {
		c.broadcast("permissionRequest", { sessionId, request });
		if (hooks.onPermissionRequest) {
			c.waitUntil(
				Promise.resolve()
					.then(() => hooks.onPermissionRequest?.(c, sessionId, request))
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
}

function releaseLiveSession(c: AnyContext, sessionId: string): void {
	const runtime = runtimeFor(c);
	runtime.sessionHolds.get(sessionId)?.();
	runtime.sessionHolds.delete(sessionId);
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
			trackLiveSession(c, vm, sessionId, hooks);
			return sessionId;
		},
		resumeSession: async (
			c: AnyContext,
			...args: Parameters<AgentOs["resumeSession"]>
		) => {
			const vm = await ensureVm(c, options);
			const result = await vm.resumeSession(...args);
			trackLiveSession(c, vm, result.sessionId, hooks);
			return result;
		},
		sendPrompt: async (c: AnyContext, sessionId: string, text: string) => {
			const promise = (await ensureVm(c, options)).prompt(sessionId, text);
			return c.keepAwake(promise);
		},
		cancelPrompt: async (c: AnyContext, sessionId: string) =>
			(await ensureVm(c, options)).cancelSession(sessionId),
		closeSession: async (c: AnyContext, sessionId: string) => {
			const vm = await ensureVm(c, options);
			vm.closeSession(sessionId);
			releaseLiveSession(c, sessionId);
		},
		destroySession: async (c: AnyContext, sessionId: string) => {
			await (await ensureVm(c, options)).destroySession(sessionId);
			releaseLiveSession(c, sessionId);
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
		listSessions: async (c: AnyContext) =>
			(await ensureVm(c, options)).listSessions(),
		setMode: async (c: AnyContext, sessionId: string, modeId: string) =>
			(await ensureVm(c, options)).setSessionMode(sessionId, modeId),
		getModes: async (c: AnyContext, sessionId: string) =>
			(await ensureVm(c, options)).getSessionModes(sessionId),
		setModel: async (c: AnyContext, sessionId: string, model: string) =>
			(await ensureVm(c, options)).setSessionModel(sessionId, model),
		setThoughtLevel: async (c: AnyContext, sessionId: string, level: string) =>
			(await ensureVm(c, options)).setSessionThoughtLevel(sessionId, level),
		getSessionConfigOptions: async (c: AnyContext, sessionId: string) =>
			(await ensureVm(c, options)).getSessionConfigOptions(sessionId),
		getSessionCapabilities: async (c: AnyContext, sessionId: string) =>
			(await ensureVm(c, options)).getSessionCapabilities(sessionId),
		getSessionAgentInfo: async (c: AnyContext, sessionId: string) =>
			(await ensureVm(c, options)).getSessionAgentInfo(sessionId),
		rawSessionSend: async (
			c: AnyContext,
			...args: Parameters<AgentOs["rawSessionSend"]>
		) => (await ensureVm(c, options)).rawSessionSend(...args),
		createSignedPreviewUrl: async (
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
				"DELETE FROM agent_os_preview_tokens WHERE expires_at <= ?",
				createdAt,
			);
			const counts = await c.db.execute<{ count: number }>(
				"SELECT COUNT(*) AS count FROM agent_os_preview_tokens",
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
		listMounts: async () => options?.mounts?.map(serializeMount) ?? [],
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

export interface AgentOsActorExtras extends AgentOsOptions {
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
	"rootFilesystem",
	"mounts",
	"additionalInstructions",
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
	const onPermissionRequest =
		actorConfig.onPermissionRequest as AgentOsEventHooks<AnyContext>["onPermissionRequest"];
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
				"DELETE FROM agent_os_preview_tokens WHERE expires_at <= ?",
				now,
			);
			const rows = await c.db.execute<{ port: number }>(
				"SELECT port FROM agent_os_preview_tokens WHERE token = ? AND expires_at > ?",
				match[1],
				now,
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
