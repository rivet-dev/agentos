import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { CONFORMANCE_AGENT_NAME } from "./agent-os-conformance-fixture.js";

export {
	CONFORMANCE_ACP_ADAPTER,
	CONFORMANCE_AGENT_NAME,
} from "./agent-os-conformance-fixture.js";

export const AGENT_OS_CONFORMANCE_ACTIONS = [
	"readFile",
	"writeFile",
	"readFiles",
	"writeFiles",
	"stat",
	"mkdir",
	"readdir",
	"readdirEntries",
	"readdirRecursive",
	"exists",
	"move",
	"remove",
	"exec",
	"execArgv",
	"spawn",
	"waitProcess",
	"killProcess",
	"stopProcess",
	"listProcesses",
	"allProcesses",
	"processTree",
	"getProcess",
	"writeProcessStdin",
	"closeProcessStdin",
	"openShell",
	"writeShell",
	"resizeShell",
	"closeShell",
	"waitShell",
	"httpRequest",
	"scheduleCron",
	"listCronJobs",
	"cancelCronJob",
	"listAgents",
	"listMounts",
	"listSoftware",
	"exportRootFilesystem",
	"mountFs",
	"unmountFs",
	"linkSoftware",
	"openSession",
	"getSession",
	"prompt",
	"cancelPrompt",
	"unloadSession",
	"deleteSession",
	"respondPermission",
	"listSessions",
	"readHistory",
	"getSessionConfig",
	"setSessionConfigOption",
	"getSessionCapabilities",
	"getSessionAgentInfo",
] as const;

export type AgentOsConformanceAction =
	(typeof AGENT_OS_CONFORMANCE_ACTIONS)[number];

export const AGENT_OS_CONFORMANCE_EVENTS = [
	"processOutput",
	"processExit",
	"shellData",
	"shellStderr",
	"shellExit",
	"cronEvent",
	"sessionEvent",
	"agentExit",
] as const;

export type AgentOsConformanceEvent =
	(typeof AGENT_OS_CONFORMANCE_EVENTS)[number];

export interface AgentOsConformanceBackend {
	call<T = unknown>(
		action: AgentOsConformanceAction,
		...args: unknown[]
	): Promise<T>;
	on(
		event: AgentOsConformanceEvent,
		handler: (payload: any) => void,
	): () => void;
	dispose(): Promise<void>;
}

export interface AgentOsConformanceOptions {
	name: string;
	skip?: boolean;
	createBackend(): Promise<AgentOsConformanceBackend>;
	verifyBackend?(backend: AgentOsConformanceBackend): Promise<void>;
}

function asBytes(value: unknown): Uint8Array {
	if (value instanceof Uint8Array) return value;
	if (
		Array.isArray(value) &&
		value[0] === "$Uint8Array" &&
		typeof value[1] === "string"
	) {
		return Buffer.from(value[1], "base64");
	}
	throw new TypeError(`expected bytes, received ${String(value)}`);
}

function text(value: unknown): string {
	return new TextDecoder().decode(asBytes(value));
}

function deferred<T>(): {
	promise: Promise<T>;
	resolve(value: T): void;
} {
	let resolve!: (value: T) => void;
	return { promise: new Promise<T>((done) => (resolve = done)), resolve };
}

async function eventually<T>(
	read: () => T | Promise<T>,
	accept: (value: T) => boolean,
	timeoutMs = 10_000,
): Promise<T> {
	const deadline = Date.now() + timeoutMs;
	let value = await read();
	while (!accept(value) && Date.now() < deadline) {
		await new Promise((resolve) => setTimeout(resolve, 25));
		value = await read();
	}
	if (!accept(value)) {
		throw new Error(`condition did not become true: ${JSON.stringify(value)}`);
	}
	return value;
}

/** Registers the complete actor-facing AgentOS contract against one backend. */
export function defineAgentOsConformanceSuite(
	options: AgentOsConformanceOptions,
): void {
	describe.skipIf(options.skip ?? false)(options.name, () => {
		let backend: AgentOsConformanceBackend;

		beforeAll(async () => {
			backend = await options.createBackend();
		}, 120_000);

		afterAll(async () => {
			await backend?.dispose();
		}, 120_000);

		test("filesystem actions preserve bytes, metadata, batches, and directory semantics", async () => {
			await backend.call("mkdir", "/conformance/fs/nested", {
				recursive: true,
			});
			await backend.call("writeFile", "/conformance/fs/a.txt", "alpha");
			await backend.call(
				"writeFile",
				"/conformance/fs/nested/b.bin",
				new Uint8Array([0, 1, 2, 255]),
			);
			expect(
				text(await backend.call("readFile", "/conformance/fs/a.txt")),
			).toBe("alpha");
			expect([
				...asBytes(
					await backend.call("readFile", "/conformance/fs/nested/b.bin"),
				),
			]).toEqual([0, 1, 2, 255]);

			const writes = await backend.call<any[]>("writeFiles", [
				{ path: "/conformance/fs/batch/c.txt", content: "charlie" },
				{ path: "/proc/conformance-denied", content: "no" },
			]);
			expect(writes.map((entry) => entry.success)).toEqual([true, false]);
			const reads = await backend.call<any[]>("readFiles", [
				"/conformance/fs/a.txt",
				"/conformance/fs/missing",
			]);
			expect(text(reads[0].content)).toBe("alpha");
			expect(reads[1].content).toBeNull();

			expect(await backend.call("exists", "/conformance/fs/a.txt")).toBe(true);
			const stat = await backend.call<any>("stat", "/conformance/fs/nested");
			expect(stat.isDirectory).toBe(true);
			expect(
				await backend.call<string[]>("readdir", "/conformance/fs"),
			).toEqual(expect.arrayContaining(["a.txt", "nested", "batch"]));
			const entries = await backend.call<any[]>(
				"readdirEntries",
				"/conformance/fs",
			);
			expect(entries).toEqual(
				expect.arrayContaining([
					expect.objectContaining({ name: "nested", isDirectory: true }),
				]),
			);
			const recursive = await backend.call<any[]>(
				"readdirRecursive",
				"/conformance/fs",
			);
			expect(recursive.map((entry) => entry.path)).toContain(
				"/conformance/fs/nested/b.bin",
			);

			await backend.call(
				"move",
				"/conformance/fs/a.txt",
				"/conformance/fs/moved.txt",
			);
			expect(await backend.call("exists", "/conformance/fs/a.txt")).toBe(false);
			await backend.call("remove", "/conformance/fs/moved.txt");
			expect(await backend.call("exists", "/conformance/fs/moved.txt")).toBe(
				false,
			);
		}, 60_000);

		test("process actions and events cover execution, inspection, stdin, stop, and kill", async () => {
			const execResult = await backend.call<any>("exec", "printf exec-ok");
			expect(execResult).toMatchObject({ exitCode: 0, stdout: "exec-ok" });
			const argvResult = await backend.call<any>("execArgv", "printf", [
				"argv-ok",
			]);
			expect(argvResult).toMatchObject({ exitCode: 0, stdout: "argv-ok" });

			const output: any[] = [];
			const exits: any[] = [];
			const offOutput = backend.on("processOutput", (event) =>
				output.push(event),
			);
			const offExit = backend.on("processExit", (event) => exits.push(event));
			const spawned = await backend.call<any>(
				"spawn",
				"node",
				[
					"-e",
					"process.stdin.on('data', d => { process.stdout.write('stdin:' + d); process.stderr.write('side'); });",
				],
				{ streamStdin: true },
			);
			expect((await backend.call<any>("getProcess", spawned.pid)).running).toBe(
				true,
			);
			expect(
				(await backend.call<any[]>("listProcesses")).some(
					(process) => process.pid === spawned.pid,
				),
			).toBe(true);
			expect(
				(await backend.call<any[]>("allProcesses")).some(
					(process) => process.pid === spawned.pid,
				),
			).toBe(true);
			expect(
				(await backend.call<any[]>("processTree")).some(
					(process) => process.pid === spawned.pid,
				),
			).toBe(true);
			await backend.call("writeProcessStdin", spawned.pid, "hello");
			await backend.call("closeProcessStdin", spawned.pid);
			expect(await backend.call("waitProcess", spawned.pid)).toBe(0);
			await eventually(
				() => output,
				(events) =>
					events.some((event) => text(event.data).includes("stdin:hello")) &&
					events.some((event) => event.stream === "stderr"),
			);
			await eventually(
				() => exits,
				(events) =>
					events.some(
						(event) => event.pid === spawned.pid && event.exitCode === 0,
					),
			);

			const stopped = await backend.call<any>("spawn", "node", [
				"-e",
				"setInterval(() => {}, 1000)",
			]);
			await backend.call("stopProcess", stopped.pid);
			await backend.call("waitProcess", stopped.pid);
			const killed = await backend.call<any>("spawn", "node", [
				"-e",
				"setInterval(() => {}, 1000)",
			]);
			await backend.call("killProcess", killed.pid);
			await backend.call("waitProcess", killed.pid);
			offOutput();
			offExit();
		}, 60_000);

		test("shell actions and events cover PTY input, resize, exit, and close", async () => {
			const data: any[] = [];
			const stderr: any[] = [];
			const exits: any[] = [];
			const offData = backend.on("shellData", (event) => data.push(event));
			const offStderr = backend.on("shellStderr", (event) =>
				stderr.push(event),
			);
			const offExit = backend.on("shellExit", (event) => exits.push(event));
			const shell = await backend.call<any>("openShell", {
				command: "node",
				args: [
					"-e",
					"process.stdin.on('data', d => { process.stdout.write('pty:' + d); process.stderr.write('pty-stderr'); process.exit(0); })",
				],
				cols: 80,
				rows: 24,
			});
			await backend.call("resizeShell", shell.shellId, 100, 30);
			await backend.call("writeShell", shell.shellId, "hello-shell\n");
			expect(await backend.call("waitShell", shell.shellId)).toBe(0);
			await eventually(
				() => data,
				(events) =>
					events.some(
						(event) =>
							event.shellId === shell.shellId &&
							text(event.data).includes("hello-shell"),
					),
			);
			await eventually(
				() => stderr,
				(events) =>
					events.some(
						(event) =>
							event.shellId === shell.shellId &&
							text(event.data).includes("pty-stderr"),
					),
			);
			await eventually(
				() => exits,
				(events) => events.some((event) => event.shellId === shell.shellId),
			);

			const closable = await backend.call<any>("openShell", {
				command: "node",
				args: ["-e", "setInterval(() => {}, 1000)"],
			});
			await backend.call("closeShell", closable.shellId);
			offData();
			offStderr();
			offExit();
		}, 60_000);

		test("network, cron, and registry actions remain serializable", async () => {
			const output: any[] = [];
			const offOutput = backend.on("processOutput", (event) =>
				output.push(event),
			);
			const server = await backend.call<any>("spawn", "node", [
				"-e",
				`
				const http = require('http');
				const server = http.createServer((req, res) => {
					let body = ''; req.on('data', chunk => body += chunk);
					req.on('end', () => { res.setHeader('x-conformance', 'yes'); res.end(req.method + ':' + req.url + ':' + body); });
				});
				server.listen(31337, '0.0.0.0', () => console.log('ready'));
			`,
			]);
			await eventually(
				() => output,
				(events) =>
					events.some(
						(event) =>
							event.pid === server.pid && text(event.data).includes("ready"),
					),
			);
			const response = await backend.call<any>(
				"httpRequest",
				{
					port: 31337,
					path: "/path?q=1",
					method: "POST",
					headers: { "content-type": "text/plain" },
					body: "payload",
				},
			);
			expect(response.status).toBe(200);
			expect(response.headers["x-conformance"]).toBe("yes");
			expect(text(response.body)).toBe("POST:/path?q=1:payload");
			await backend.call("killProcess", server.pid);
			await backend.call("waitProcess", server.pid);
			offOutput();

			const job = await backend.call<any>("scheduleCron", {
				id: "conformance-cron",
				schedule: "0 0 1 1 *",
				action: { type: "exec", command: "node", args: ["-e", "void 0"] },
				overlap: "skip",
			});
			expect(job.id).toBe("conformance-cron");
			expect(await backend.call<any[]>("listCronJobs")).toEqual([
				expect.objectContaining({ id: "conformance-cron", overlap: "skip" }),
			]);
			await backend.call("cancelCronJob", job.id);
			expect(await backend.call<any[]>("listCronJobs")).toEqual([]);
			const cronEvents: any[] = [];
			const offCron = backend.on("cronEvent", (event) =>
				cronEvents.push(event),
			);
			const oneShot = await backend.call<any>("scheduleCron", {
				id: "conformance-cron-event",
				schedule: new Date(Date.now() + 750).toISOString(),
				action: { type: "exec", command: "node", args: ["-e", "void 0"] },
			});
			await eventually(
				() => cronEvents,
				(events) =>
					events.some(
						(payload) =>
							payload.type === "cron:complete" && payload.jobId === oneShot.id,
					),
				10_000,
			);
			await backend.call("cancelCronJob", oneShot.id);
			offCron();

			const agents = await backend.call<any[]>("listAgents");
			expect(agents).toEqual(
				expect.arrayContaining([
					expect.objectContaining({
						id: CONFORMANCE_AGENT_NAME,
						installed: true,
					}),
				]),
			);
			expect(await backend.call<any[]>("listMounts")).toContainEqual(
				expect.objectContaining({
					path: "/conformance-mount",
					kind: "host_dir",
					readOnly: true,
				}),
			);
			expect(
				text(
					await backend.call(
						"readFile",
						"/conformance-mount/package.json",
					),
				),
			).toContain(CONFORMANCE_AGENT_NAME);
			await expect(
				backend.call(
					"writeFile",
					"/conformance-mount/should-fail.txt",
					"read-only",
				),
			).rejects.toThrow();
			const software = await backend.call<any[]>("listSoftware");
			expect(
				software.some((entry) =>
					entry.commands.includes("conformance-agent-acp"),
				),
			).toBe(true);
		}, 60_000);

		test("sessions cover durable history, live events, permission replies, config, restoration, unload, and deletion", async () => {
			const sessionId = "conformance-session";
			expect(
				await backend.call("openSession", {
					sessionId,
					agent: CONFORMANCE_AGENT_NAME,
					permissionPolicy: "ask",
					cwd: "/workspace",
					env: { CONFORMANCE_INPUT: "present" },
					skipOsInstructions: true,
					additionalInstructions: "shared-suite",
				}),
			).toBeUndefined();
			expect((await backend.call<any>("listSessions")).sessions).toContainEqual(
				expect.objectContaining({
					sessionId,
					agent: CONFORMANCE_AGENT_NAME,
				}),
			);
			expect(
				(await backend.call<any>("getSessionConfig", { sessionId })).options,
			).toHaveLength(2);
			expect(
				await backend.call<any>("getSessionCapabilities", { sessionId }),
			).toMatchObject({ loadSession: true });
			expect(
				await backend.call<any>("getSessionAgentInfo", { sessionId }),
			).toMatchObject({ name: CONFORMANCE_AGENT_NAME });

			const sessionEvents: any[] = [];
			const permissions: any[] = [];
			const permissionReady = deferred<any>();
			const offSession = backend.on("sessionEvent", (event) => {
				sessionEvents.push(event);
				if (
					event.sessionId === sessionId &&
					event.type === "permission_request"
				) {
					permissions.push(event);
					permissionReady.resolve(event);
				}
			});
			const prompt = backend.call<any>("prompt", {
				sessionId,
				content: [{ type: "text", text: "permission please" }],
			});
			await eventually(
				() => sessionEvents,
				(events) =>
					events.some(
						(event) =>
							event.sessionId === sessionId &&
							event.type === "agent_message_chunk",
					),
			);
			const permission = await Promise.race([
				permissionReady.promise,
				new Promise<never>((_, reject) =>
					setTimeout(
						() => reject(new Error("permission request timed out")),
						10_000,
					),
				),
			]);
			expect(permission.toolCall.toolCallId).toBe("binding-call-1");
			await backend.call("respondPermission", {
				sessionId,
				requestId: permission.requestId,
				optionId: "allow_once",
			});
			expect(JSON.stringify((await prompt).message)).toContain("permission");
			expect(permissions).toHaveLength(1);

			await backend.call("setSessionConfigOption", {
				sessionId,
				configId: "model",
				value: "next-model",
			});
			await backend.call("setSessionConfigOption", {
				sessionId,
				configId: "thought_level",
				value: "high",
			});
			const config = (await backend.call<any>("getSessionConfig", { sessionId }))
				.options;
			expect(
				config.find((entry: any) => entry.category === "model")?.currentValue,
			).toBe("next-model");
			expect(
				config.find((entry: any) => entry.category === "thought_level")
					?.currentValue,
			).toBe("high");
			await backend.call("cancelPrompt", { sessionId });
			const history = await backend.call<any>("readHistory", { sessionId });
			expect(history.events.length).toBeGreaterThan(0);
			const permissionHistory = history.events.filter(
				(entry: any) =>
					entry.type === "permission_request" ||
					entry.type === "permission_response",
			);
			expect(permissionHistory.map((entry: any) => entry.type)).toEqual([
				"permission_request",
				"permission_response",
			]);
			expect(permissionHistory[0].sequence).toBeLessThan(
				permissionHistory[1].sequence,
			);
			expect(permission.sequence).toBe(permissionHistory[0].sequence);
			expect(
				sessionEvents.some(
					(event) =>
						event.type === "permission_response" &&
						event.sequence === permissionHistory[1].sequence,
				),
			).toBe(true);
			const recoveredBySequence = new Map<number, any>();
			for (const entry of [...history.events, ...sessionEvents]) {
				if (entry?.durability === "durable" || entry?.sequence !== undefined) {
					recoveredBySequence.set(entry.sequence, entry);
				}
			}
			expect(recoveredBySequence.size).toBe(history.events.length);

			await backend.call("unloadSession", { sessionId });
			const restored = await backend.call<any>("prompt", {
				sessionId,
				content: [{ type: "text", text: "restored" }],
			});
			expect(JSON.stringify(restored.message)).toContain("restored");
			await backend.call("deleteSession", { sessionId });
			expect((await backend.call<any>("listSessions")).sessions).not.toContainEqual(
				expect.objectContaining({ sessionId }),
			);
			offSession();
		}, 90_000);

		test("default permission policy auto-resolves without durable or live permission events", async () => {
			const sessionId = "conformance-auto-permission";
			const sessionEvents: any[] = [];
			const offSession = backend.on("sessionEvent", (event) => {
				if (event.sessionId === sessionId) sessionEvents.push(event);
			});
			try {
				// permissionPolicy is deliberately omitted: the sidecar-owned default
				// must be allow_all for both Core and actor clients.
				expect(
					await backend.call("openSession", {
						sessionId,
						agent: CONFORMANCE_AGENT_NAME,
						skipOsInstructions: true,
					}),
				).toBeUndefined();
				const result = await backend.call<any>("prompt", {
					sessionId,
					content: [{ type: "text", text: "permission automatically" }],
				});
				expect(JSON.stringify(result.message)).toContain("allow_once");

				const history = await backend.call<any>("readHistory", { sessionId });
				expect(
					history.events.filter(
						(entry: any) =>
							entry.type === "permission_request" ||
							entry.type === "permission_response",
					),
				).toEqual([]);
				expect(
					sessionEvents.some(
						(event) =>
							event.type === "permission_request" ||
							event.type === "permission_response",
					),
				).toBe(false);
			} finally {
				offSession();
				await backend.call("deleteSession", { sessionId });
			}
		}, 90_000);

		test("unexpected ACP adapter exits surface through agentExit", async () => {
			const crashes: any[] = [];
			const offCrash = backend.on("agentExit", (event) =>
				crashes.push(event),
			);
			const sessionId = "crash-session";
			await backend.call("openSession", {
				sessionId,
				agent: CONFORMANCE_AGENT_NAME,
				skipOsInstructions: true,
			});
			await backend
				.call("prompt", {
					sessionId,
					content: [{ type: "text", text: "crash-adapter" }],
				})
				.catch(() => undefined);
			await eventually(
				() => crashes,
				(events) => events.some((event) => event.sessionId === sessionId),
				15_000,
			);
			await backend.call("deleteSession", { sessionId });
			offCrash();
		}, 30_000);

		if (options.verifyBackend) {
			test("backend-specific integration hooks observe the shared contract", async () => {
				await options.verifyBackend?.(backend);
			});
		}
	});
}
