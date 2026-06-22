/**
 * Type-checking example for `@rivet-dev/agentos`.
 *
 * This file exists to EXERCISE the exported type surface of the interim
 * types-now / noop-runtime `@rivet-dev/agentos` package so that `tsc` would
 * catch a broken public type. It is NOT meant to run: the stubbed `agentOS()`
 * action handlers throw `"agent-os runtime not yet wired — dylib pending"` at
 * runtime. That is expected and fine for a type-check fixture.
 */

import {
	agentOS,
	nodeModulesMount,
	setup,
	type BatchReadResult,
	type BatchWriteResult,
	type CronJobInfo,
	type DirEntry,
	type ExecResult,
	type NodeModulesMountConfig,
	type PermissionReply,
	type PersistedSessionEvent,
	type PersistedSessionRecord,
	type PreviewUrl,
	type PromptResult,
	type RootSnapshotExport,
	type SequencedSessionEvent,
	type SerializableCronJobOptions,
	type SessionConfigOption,
	type SessionModeState,
	type SessionRecord,
	type SpawnedProcessInfo,
	type VirtualStat,
	type VmFetchResponse,
} from "@rivet-dev/agentos";
import { createClient } from "@rivet-dev/agentos/client";
import pi from "./software/pi";

// 1. Build a node_modules mount descriptor (exercises the helper + its type).
const mount: NodeModulesMountConfig = nodeModulesMount("/abs/host/node_modules", {
	readOnly: true,
});

// 2. Define the agent-os actor via the stub factory. Config is FLAT: the VM
//    option fields (software, mounts, instructions, ...) sit at the top level
//    alongside preview + the event callbacks. `software` is an array of
//    imported software packages.
const vm = agentOS({
	software: [pi],
	mounts: [mount],
	additionalInstructions: "Be concise.",
	allowedNodeBuiltins: ["path", "fs"],
	loopbackExemptPorts: [3000],
	preview: {
		defaultExpiresInSeconds: 3600,
		maxExpiresInSeconds: 86_400,
	},
	onSessionEvent: (sessionId, event) => {
		// `event` is a JsonRpcNotification.
		console.log(sessionId, event.method);
	},
	onPermissionRequest: async (sessionId, request) => {
		console.log(sessionId, request.permissionId);
	},
});

// 3. Register the actor and create a typed client.
const registry = setup({ use: { vm } });
const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

async function main(): Promise<void> {
	const handle = client.vm.getOrCreate("my-agent");

	// 4. Sessions: create / prompt / config / cancel / lifecycle.
	const session: SessionRecord = await handle.createSession("claude", {
		cwd: "/work",
		env: { ANTHROPIC_API_KEY: "sk-ant-..." },
		mcpServers: [
			{ type: "local", command: "npx", args: ["-y", "@modelcontextprotocol/server-filesystem", "/work"] },
			{ type: "remote", url: "https://mcp.example.com/sse", headers: { Authorization: "Bearer token" } },
		],
		additionalInstructions: "Be concise.",
	});
	const result: PromptResult = await handle.sendPrompt(
		session.sessionId,
		"List the files in the working directory.",
	);
	console.log(result.text, result.response.jsonrpc);
	await handle.prompt(session.sessionId, "Now summarize them.");
	await handle.cancelPrompt(session.sessionId);

	await handle.setModel(session.sessionId, "claude-sonnet-4-6");
	await handle.setMode(session.sessionId, "plan");
	await handle.setThoughtLevel(session.sessionId, "high");
	const modes: SessionModeState | null = await handle.getModes(session.sessionId);
	console.log(modes?.currentModeId);
	const options: SessionConfigOption[] = await handle.getConfigOptions(
		session.sessionId,
	);
	console.log(options.length);

	const sessions: SessionRecord[] = await handle.listSessions();
	console.log(sessions.length);

	// 5. Permission approval.
	const reply: PermissionReply = "once";
	await handle.respondPermission(session.sessionId, "perm-1", reply);

	// 6. Session event replay (in-memory + persisted).
	const sequenced: SequencedSessionEvent[] = await handle.getSequencedEvents(
		session.sessionId,
		{ since: 0 },
	);
	for (const e of sequenced) {
		console.log(e.sequenceNumber, e.notification.method);
	}
	const persisted: PersistedSessionEvent[] = await handle.getSessionEvents(
		session.sessionId,
	);
	for (const e of persisted) {
		console.log(e.seq, e.event.method, e.createdAt);
	}
	const persistedSessions: PersistedSessionRecord[] =
		await handle.listPersistedSessions();
	for (const s of persistedSessions) {
		console.log(s.sessionId, s.agentType, s.createdAt);
	}

	// 7. Processes: exec, spawn, lifecycle.
	const execResult: ExecResult = await handle.exec("echo hi && ls /work", {
		cwd: "/work",
	});
	console.log(execResult.stdout, execResult.exitCode);
	await handle.execArgv("node", ["--version"]);

	const { pid } = await handle.spawn("node", ["/work/server.js"], {
		env: { PORT: "3000" },
	});
	const procs: SpawnedProcessInfo[] = await handle.listProcesses();
	console.log(procs.length);
	const info: SpawnedProcessInfo = await handle.getProcess(pid);
	console.log(info.running, info.exitCode);
	await handle.writeProcessStdin(pid, "hello\n");
	await handle.closeProcessStdin(pid);
	const code: number = await handle.waitProcess(pid);
	console.log(code);
	await handle.stopProcess(pid);
	await handle.killProcess(pid);

	const legacyPid: number = await handle.spawnProcess("ls", ["-la"]);
	await handle.killProcess(legacyPid);

	// 8. Shells / PTYs.
	const { shellId } = await handle.openShell({ cols: 120, rows: 40 });
	await handle.writeShell(shellId, "echo hi\n");
	await handle.resizeShell(shellId, 100, 30);
	await handle.closeShell(shellId);
	await handle.connectTerminal({ command: "/bin/sh" });

	// 9. Filesystem: read/write/batch/mkdir/readdir/stat/exists/move/delete.
	await handle.writeFile("/work/hello.txt", "hi");
	const bytes: Uint8Array = await handle.readFile("/work/hello.txt");
	console.log(bytes.byteLength);
	const writes: BatchWriteResult[] = await handle.writeFiles([
		{ path: "/work/a.txt", content: "a" },
		{ path: "/work/b.txt", content: new Uint8Array([1, 2]) },
	]);
	console.log(writes.every((w) => w.success));
	const reads: BatchReadResult[] = await handle.readFiles([
		"/work/a.txt",
		"/work/b.txt",
	]);
	console.log(reads.map((r) => r.content?.byteLength));
	await handle.mkdir("/work/sub", { recursive: true });
	const names: string[] = await handle.readdir("/work");
	console.log(names);
	const tree: DirEntry[] = await handle.readdirRecursive("/work", {
		maxDepth: 2,
		exclude: ["node_modules"],
	});
	console.log(tree.map((d) => `${d.type}:${d.path}`));
	const st: VirtualStat = await handle.stat("/work/hello.txt");
	console.log(st.size, st.isDirectory);
	console.log(await handle.exists("/work/hello.txt"));
	await handle.move("/work/a.txt", "/work/c.txt");
	await handle.delete("/work/sub", { recursive: true });
	const snapshot: RootSnapshotExport = await handle.snapshotRootFilesystem();
	console.log(snapshot.source.filesystem.entries.length);

	// 10. Networking + preview URLs.
	const response: VmFetchResponse = await handle.vmFetch(3000, "/api/data", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ key: "value" }),
	});
	console.log(response.status, new TextDecoder().decode(response.body));
	const preview: PreviewUrl = await handle.createSignedPreviewUrl(3000, 300);
	console.log(preview.path, preview.token, new Date(preview.expiresAt));

	// 11. Cron.
	const cron: SerializableCronJobOptions = {
		schedule: "*/5 * * * *",
		action: { type: "exec", command: "echo", args: ["tick"] },
		overlap: "skip",
	};
	const { id } = await handle.scheduleCron(cron);
	console.log(id);
	await handle.scheduleCron({
		schedule: "0 9 * * *",
		action: {
			type: "session",
			agentType: "claude",
			prompt: "Summarize the logs",
			options: { cwd: "/work" },
		},
	});
	const jobs: CronJobInfo[] = await handle.listCronJobs();
	for (const job of jobs) {
		console.log(job.id, job.schedule);
	}
	if (jobs[0]) {
		await handle.cancelCronJob(jobs[0].id);
	}

	await handle.cancelSession(session.sessionId);
	await handle.destroySession(session.sessionId);
	await handle.closeSession(session.sessionId);

	// 12. Subscribe to events over a live connection. The event NAMES are
	//     constrained by the actor's event schema (a typo here is a tsc error);
	//     each payload is INFERRED from the schema, so no casts are needed.
	const conn = handle.connect();
	const unsubscribe = conn.on("sessionEvent", (payload) => {
		console.log(payload.sessionId, payload.event.method);
	});
	conn.on("permissionRequest", (payload) => {
		console.log(payload.sessionId, payload.request.permissionId);
	});
	conn.on("processOutput", (payload) => {
		console.log(payload.pid, payload.stream, payload.data.byteLength);
	});
	conn.on("processExit", (payload) => {
		console.log(payload.pid, payload.exitCode);
	});
	conn.on("shellData", (payload) => {
		console.log(payload.shellId, payload.data.byteLength);
	});
	conn.on("cronEvent", (payload) => {
		console.log(payload.event.id, payload.event.schedule);
	});
	unsubscribe();
}

// Exercised at type level only; the stub throws at runtime by design.
export { main };
