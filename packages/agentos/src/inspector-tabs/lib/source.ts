import { keepPreviousData, queryOptions } from "@tanstack/react-query";
import type { AgentOsActorHandle, Output } from "../../generated/contract";
import { getAgentOsHandle, runInspectorAction } from "./actor-client";
import type {
	FileContent,
	FsEntry,
	MountInfo,
	ProcessTreeNode,
	RuntimeHealth,
	ShellInfo,
	ShellReplayMode,
	ShellSnapshot,
	SignedPreviewUrl,
	SoftwareBundle,
} from "./types";

export const agentOsQueryKey = (actorId: string, ...rest: string[]) => [
	"agentOS",
	actorId,
	...rest,
];

export function decodeActionBytes(output: unknown): Uint8Array {
	if (output instanceof Uint8Array) return output;
	if (
		Array.isArray(output) &&
		output[0] === "$Uint8Array" &&
		typeof output[1] === "string"
	) {
		const binary = atob(output[1]);
		return Uint8Array.from(binary, (character) => character.charCodeAt(0));
	}
	if (Array.isArray(output)) return Uint8Array.from(output as number[]);
	if (typeof output === "string") return new TextEncoder().encode(output);
	return new Uint8Array();
}

function joinPath(directory: string, name: string): string {
	return directory === "/" ? `/${name}` : `${directory}/${name}`;
}

function bytesToDisplay(bytes: Uint8Array): string | null {
	if (bytes.subarray(0, 8192).includes(0)) return null;
	return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
}

function softwareBundle(
	software: Output.ActorInstalledSoftware,
): SoftwareBundle {
	const slug = software.packageName.split("/").at(-1) ?? software.packageName;
	return {
		name: software.packageName,
		slug: slug.toLowerCase(),
		version: software.version,
		source: software.packageName.startsWith("@agentos-software/")
			? "rivet-dev"
			: "user",
		binaries: software.commands,
	};
}

function processNode(
	node: Output.ActorProcessTreeNode,
	generation: number | bigint,
): ProcessTreeNode {
	return {
		generation,
		process: node.process ?? null,
		pid: node.pid,
		ppid: node.ppid,
		pgid: node.pgid,
		sid: node.sid,
		driver: node.driver,
		command: node.command,
		args: node.args,
		cwd: node.cwd,
		status: node.status,
		exitCode: node.exit?.exitCode ?? null,
		startTime: node.startTimeMs,
		exitTime: node.exitTimeMs ?? null,
		children: node.children.map((child) => processNode(child, generation)),
	};
}

const MAX_FILE_PREVIEW_BYTES = 700 * 1024;
const MAX_TERMINAL_SNAPSHOT_EVENTS_PER_PAGE = 256;
const MAX_TERMINAL_SNAPSHOT_PAGE_BYTES = 700 * 1024;
const MAX_TERMINAL_SNAPSHOT_PAGES = 8;
const MAX_TERMINAL_SNAPSHOT_BYTES = 2 * 1024 * 1024;
export const PROCESS_REPLAY_PAGE_LIMITS = {
	maxEvents: 256,
	maxBytes: 64 * 1024,
} as const;

export const agentOsSource = {
	softwareQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: agentOsQueryKey(actorId, "software"),
			queryFn: async () =>
				(
					await runInspectorAction("software.list", (actor) =>
						actor.software.list({}),
					)
				).map(softwareBundle),
		}),

	processTreeQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: agentOsQueryKey(actorId, "process-tree"),
			queryFn: async () => {
				const tree = await runInspectorAction("process.tree", (actor) =>
					actor.process.tree({}),
				);
				return tree.roots.map((node) => processNode(node, tree.generation));
			},
			refetchInterval: 5_000,
		}),

	listDirQueryOptions: (actorId: string, path: string, enabled = true) =>
		queryOptions({
			queryKey: agentOsQueryKey(actorId, "dir", path),
			enabled,
			queryFn: async (): Promise<FsEntry[]> => {
				const entries = await runInspectorAction(
					"filesystem.readdirEntries",
					(actor) => actor.filesystem.readdirEntries({ path }),
				);
				return entries
					.filter((entry) => entry.name !== "." && entry.name !== "..")
					.map((entry) => ({
						name: entry.name,
						path: joinPath(path, entry.name),
						dir: entry.isDirectory,
						symlink: entry.isSymbolicLink,
					}))
					.sort(
						(left, right) =>
							Number(right.dir) - Number(left.dir) ||
							left.name.localeCompare(right.name),
					);
			},
		}),

	fileContentQueryOptions: (actorId: string, path: string | null) =>
		queryOptions({
			queryKey: agentOsQueryKey(actorId, "file", path ?? ""),
			enabled: path !== null,
			placeholderData: keepPreviousData,
			queryFn: async (): Promise<FileContent> => {
				const selectedPath = path as string;
				const stat = await runInspectorAction("filesystem.stat", (actor) =>
					actor.filesystem.stat({ path: selectedPath }),
				);
				const sizeBytes = Number(stat.sizeBytes);
				const fileType = stat.mode & 0o170000;
				if ([0o020000, 0o060000, 0o010000, 0o140000].includes(fileType)) {
					return {
						path: selectedPath,
						sizeBytes,
						mtimeMs: stat.mtimeMs,
						text: null,
						bytes: null,
						oversize: false,
						special: true,
					};
				}
				if (sizeBytes > MAX_FILE_PREVIEW_BYTES) {
					return {
						path: selectedPath,
						sizeBytes,
						mtimeMs: stat.mtimeMs,
						text: null,
						bytes: null,
						oversize: true,
					};
				}
				const bytes = decodeActionBytes(
					await runInspectorAction("filesystem.readFile", (actor) =>
						actor.filesystem.readFile({
							path: selectedPath,
							maxBytes: MAX_FILE_PREVIEW_BYTES,
						}),
					),
				);
				return {
					path: selectedPath,
					sizeBytes,
					mtimeMs: stat.mtimeMs,
					text: bytesToDisplay(bytes),
					bytes,
					oversize: false,
				};
			},
		}),

	mountsQueryOptions: (actorId: string) =>
		queryOptions({
			queryKey: agentOsQueryKey(actorId, "mounts"),
			queryFn: async (): Promise<MountInfo[]> =>
				runInspectorAction("filesystem.listMounts", (actor) =>
					actor.filesystem.listMounts({}),
				),
		}),

	stopProcess: async (generation: number | bigint, pid: number) => {
		return runInspectorAction("process.signal", (actor) =>
			actor.process.signal({
				process: { generation, pid },
				signal: "SIGTERM",
			}),
		);
	},

	processOutputReader: (generation: number | bigint, pid: number) => {
		// A multi-page drain must not switch actors between requests.
		const actor = getAgentOsHandle();
		return (after?: number | bigint) =>
			runInspectorAction(
				"process.output.read",
				(handle) =>
					handle.process.output.read({
						process: { generation, pid },
						after,
						...PROCESS_REPLAY_PAGE_LIMITS,
					}),
				actor,
			);
	},

	killProcess: async (generation: number | bigint, pid: number) => {
		return runInspectorAction("process.signal", (actor) =>
			actor.process.signal({
				process: { generation, pid },
				signal: "SIGKILL",
			}),
		);
	},

	openShell: async (cols: number, rows: number) => {
		const actor = getAgentOsHandle();
		const terminal = await runInspectorAction(
			"terminal.open",
			(handle) =>
				handle.terminal.open({ options: { args: [], env: {}, cols, rows } }),
			actor,
		);
		terminalIdsFor(actor).set(terminal.shellId, terminal);
		return terminal;
	},

	writeShell: async (shellId: string, data: string) => {
		const actor = getAgentOsHandle();
		const terminal = await terminalId(actor, shellId);
		return runInspectorAction(
			"terminal.stdin.write",
			(handle) => handle.terminal.stdin.write({ terminal, data }),
			actor,
		);
	},

	resizeShell: async (shellId: string, cols: number, rows: number) => {
		const actor = getAgentOsHandle();
		const terminal = await terminalId(actor, shellId);
		return runInspectorAction(
			"terminal.pty.resize",
			(handle) => handle.terminal.pty.resize({ terminal, cols, rows }),
			actor,
		);
	},

	closeShell: async (shellId: string) => {
		const actor = getAgentOsHandle();
		const terminal = await terminalId(actor, shellId);
		const closed = await runInspectorAction(
			"terminal.close",
			(handle) => handle.terminal.close({ terminal }),
			actor,
		);
		terminalIdsFor(actor).delete(shellId);
		return closed;
	},

	shellSnapshot: async (
		shellId: string,
		mode: ShellReplayMode = "screen",
		signal?: AbortSignal,
	): Promise<ShellSnapshot> => {
		signal?.throwIfAborted();
		// Keep all pages on the same actor if the inspector reconnects mid-read.
		const actor = getAgentOsHandle();
		const terminal = await terminalId(actor, shellId);
		const chunks: Uint8Array[] = [];
		let bytes = 0;
		let cursor: number | bigint | undefined;
		let hasMore = false;
		let truncated = false;
		for (let page = 0; page < MAX_TERMINAL_SNAPSHOT_PAGES; page++) {
			signal?.throwIfAborted();
			const replay = await runInspectorAction(
				"terminal.output.read",
				(handle) =>
					handle.terminal.output.read({
						terminal,
						after: cursor,
						maxBytes: MAX_TERMINAL_SNAPSHOT_PAGE_BYTES,
						maxEvents: MAX_TERMINAL_SNAPSHOT_EVENTS_PER_PAGE,
					}),
				actor,
			);
			signal?.throwIfAborted();
			truncated ||= replay.truncated;
			if (replay.events.length > MAX_TERMINAL_SNAPSHOT_EVENTS_PER_PAGE) {
				throw new Error(
					"Terminal replay exceeded the requested event page limit",
				);
			}
			let pageBytes = 0;
			let lastSequence = cursor;
			for (const event of replay.events) {
				if (
					event.sequence < 0 ||
					(typeof event.sequence === "number" &&
						!Number.isSafeInteger(event.sequence)) ||
					(lastSequence !== undefined && event.sequence <= lastSequence)
				) {
					throw new Error("Terminal replay returned out-of-order sequences");
				}
				lastSequence = event.sequence;
				const chunk = decodeActionBytes(event.data);
				pageBytes += chunk.byteLength;
				if (pageBytes > MAX_TERMINAL_SNAPSHOT_PAGE_BYTES) {
					throw new Error(
						"Terminal replay exceeded the requested byte page limit",
					);
				}
				bytes += chunk.byteLength;
				if (bytes > MAX_TERMINAL_SNAPSHOT_BYTES) {
					throw new Error(
						"Terminal snapshot exceeds the 2 MiB inspector limit",
					);
				}
				chunks.push(chunk);
			}
			const nextCursor = replay.nextCursor ?? undefined;
			if (
				(nextCursor !== undefined &&
					(nextCursor < 0 ||
						(typeof nextCursor === "number" &&
							!Number.isSafeInteger(nextCursor)))) ||
				(replay.hasMore && replay.events.length === 0) ||
				(replay.events.length > 0 && nextCursor === undefined) ||
				(nextCursor !== undefined &&
					BigInt(nextCursor) !== BigInt(lastSequence ?? -1) &&
					!(
						replay.truncated &&
						!replay.hasMore &&
						BigInt(nextCursor) > BigInt(lastSequence ?? -1)
					))
			) {
				throw new Error("Terminal replay did not advance its cursor");
			}
			cursor = nextCursor ?? cursor;
			hasMore = replay.hasMore;
			if (!hasMore) break;
		}
		if (hasMore) {
			throw new Error("Terminal snapshot exceeds the 8-page inspector limit");
		}
		return {
			shellId,
			mode,
			seq: cursor ?? -1,
			truncated,
			cols: 80,
			rows: 24,
			// The last replay event can end halfway through a UTF-8 character.
			// Let the terminal decode it together with subsequent live bytes.
			data: concatBytes(chunks),
		};
	},

	writeFile: (path: string, content: Uint8Array | string) =>
		runInspectorAction("filesystem.writeFile", (actor) =>
			actor.filesystem.writeFile({ path, content }),
		),

	mkdir: (path: string) =>
		runInspectorAction("filesystem.mkdir", (actor) =>
			actor.filesystem.mkdir({ path, recursive: true }),
		),

	moveEntry: (from: string, to: string) =>
		runInspectorAction("filesystem.move", (actor) =>
			actor.filesystem.move({ from, to }),
		),

	deleteFile: (path: string, options: { recursive?: boolean }) =>
		runInspectorAction("filesystem.remove", (actor) =>
			actor.filesystem.remove({ path, recursive: options.recursive ?? false }),
		),

	createSignedPreviewUrl: async (
		port: number,
		ttlSeconds: number,
	): Promise<SignedPreviewUrl> => {
		const preview = await runInspectorAction(
			"network.preview.create",
			(actor) =>
				actor.network.preview.create({ port, ttlMs: ttlSeconds * 1000 }),
		);
		return {
			path: preview.path,
			token: preview.token,
			port: preview.port,
			expiresAt: Number(preview.expiresAtMs),
		};
	},

	expireSignedPreviewUrl: (token: string) =>
		runInspectorAction("network.preview.expire", (actor) =>
			actor.network.preview.expire({ token }),
		),
};

async function terminalId(
	actor: AgentOsActorHandle,
	shellId: string,
): Promise<Output.ActorTerminalId> {
	const cached = terminalIdsFor(actor).get(shellId);
	if (cached) return cached;
	const terminals = await listTerminals(actor);
	const terminal = terminals.find(
		(entry) => entry.terminal.shellId === shellId,
	);
	if (!terminal) throw new Error(`terminal ${shellId} is no longer available`);
	return terminal.terminal;
}

// A shell ID is only meaningful inside its actor. Weak keys also release old
// caches when reconnecting creates a new actor handle.
const terminalIds = new WeakMap<
	AgentOsActorHandle,
	Map<string, Output.ActorTerminalId>
>();

function terminalIdsFor(
	actor: AgentOsActorHandle,
): Map<string, Output.ActorTerminalId> {
	let ids = terminalIds.get(actor);
	if (!ids) {
		ids = new Map();
		terminalIds.set(actor, ids);
	}
	return ids;
}

async function listTerminals(actor: AgentOsActorHandle) {
	const terminals = await runInspectorAction(
		"terminal.list",
		(handle) => handle.terminal.list({}),
		actor,
	);
	// Replace rather than accumulate IDs of terminals closed by another client.
	terminalIds.set(
		actor,
		new Map(terminals.map(({ terminal }) => [terminal.shellId, terminal])),
	);
	return terminals;
}

function concatBytes(chunks: Uint8Array[]): Uint8Array {
	const output = new Uint8Array(
		chunks.reduce((total, chunk) => total + chunk.byteLength, 0),
	);
	let offset = 0;
	for (const chunk of chunks) {
		output.set(chunk, offset);
		offset += chunk.byteLength;
	}
	return output;
}

export const shellsQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: agentOsQueryKey(actorId, "terminals"),
		queryFn: async (): Promise<ShellInfo[]> => {
			const actor = getAgentOsHandle();
			return (await listTerminals(actor)).map((entry) => ({
				shellId: entry.terminal.shellId,
				openedAt: 0,
			}));
		},
		refetchInterval: 10_000,
	});

export const healthQueryOptions = (actorId: string) =>
	queryOptions({
		queryKey: agentOsQueryKey(actorId, "vm-status"),
		queryFn: (): Promise<RuntimeHealth> =>
			runInspectorAction("vm.status", (actor) => actor.vm.status({})),
		refetchInterval: 5_000,
	});
