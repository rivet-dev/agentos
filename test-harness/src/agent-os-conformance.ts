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
	"deleteFile",
	"exec",
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
	"vmFetch",
	"scheduleCron",
	"listCronJobs",
	"cancelCronJob",
	"listAgents",
	"listMounts",
	"listSoftware",
	"createSession",
	"resumeSession",
	"sendPrompt",
	"cancelPrompt",
	"closeSession",
	"destroySession",
	"respondPermission",
	"listSessions",
	"setMode",
	"getModes",
	"setModel",
	"setThoughtLevel",
	"getSessionConfigOptions",
	"getSessionCapabilities",
	"getSessionAgentInfo",
	"rawSessionSend",
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
	"permissionRequest",
	"agentCrashed",
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
	if (!accept(value)) throw new Error("condition did not become true");
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
			await backend.call("deleteFile", "/conformance/fs/moved.txt");
			expect(await backend.call("exists", "/conformance/fs/moved.txt")).toBe(
				false,
			);
		}, 60_000);

		test("process actions and events cover execution, inspection, stdin, stop, and kill", async () => {
			const execResult = await backend.call<any>("exec", "printf exec-ok");
			expect(execResult).toMatchObject({ exitCode: 0, stdout: "exec-ok" });

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
				"vmFetch",
				31337,
				"http://agentos.test/path?q=1",
				{
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
							(payload.event ?? payload).type === "cron:complete" &&
							(payload.event ?? payload).jobId === oneShot.id,
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

		test("sessions cover creation, live events, permission replies, config, raw RPC, resume, close, and destroy", async () => {
			const sessionId = await backend.call<string>(
				"createSession",
				CONFORMANCE_AGENT_NAME,
				{
					cwd: "/workspace",
					env: { CONFORMANCE_INPUT: "present" },
					skipOsInstructions: true,
					additionalInstructions: "shared-suite",
				},
			);
			expect(typeof sessionId).toBe("string");
			expect(await backend.call<any[]>("listSessions")).toContainEqual(
				expect.objectContaining({
					sessionId,
					agentType: CONFORMANCE_AGENT_NAME,
				}),
			);
			expect(await backend.call<any>("getModes", sessionId)).toMatchObject({
				currentModeId: "default",
			});
			expect(
				await backend.call<any[]>("getSessionConfigOptions", sessionId),
			).toHaveLength(2);
			expect(
				await backend.call<any>("getSessionCapabilities", sessionId),
			).toMatchObject({ loadSession: true });
			expect(
				await backend.call<any>("getSessionAgentInfo", sessionId),
			).toMatchObject({ name: CONFORMANCE_AGENT_NAME });

			const sessionEvents: any[] = [];
			const permissions: any[] = [];
			const permissionReady = deferred<any>();
			const offSession = backend.on("sessionEvent", (event) =>
				sessionEvents.push(event),
			);
			const offPermission = backend.on("permissionRequest", (event) => {
				permissions.push(event);
				if (event.sessionId === sessionId) permissionReady.resolve(event);
			});
			const prompt = backend.call<any>(
				"sendPrompt",
				sessionId,
				"permission please",
			);
			await eventually(
				() => sessionEvents,
				(events) =>
					events.some(
						(event) =>
							event.sessionId === sessionId &&
							event.event?.method === "session/update",
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
			expect(permission.request.params.toolCall.toolCallId).toBe(
				"binding-call-1",
			);
			await backend.call(
				"respondPermission",
				sessionId,
				permission.request.permissionId,
				"once",
			);
			expect((await prompt).text).toContain("permission");
			expect(permissions).toHaveLength(1);

			await backend.call("setMode", sessionId, "plan");
			await backend.call("setModel", sessionId, "next-model");
			await backend.call("setThoughtLevel", sessionId, "high");
			expect(await backend.call<any>("getModes", sessionId)).toMatchObject({
				currentModeId: "plan",
			});
			const config = await backend.call<any[]>(
				"getSessionConfigOptions",
				sessionId,
			);
			expect(
				config.find((entry) => entry.category === "model")?.currentValue,
			).toBe("next-model");
			expect(
				config.find((entry) => entry.category === "thought_level")
					?.currentValue,
			).toBe("high");
			expect(
				await backend.call("rawSessionSend", sessionId, "conformance/echo", {
					value: 42,
				}),
			).toMatchObject({ result: { echoed: { value: 42 } } });
			await backend.call("cancelPrompt", sessionId);

			const resumed = await backend.call<any>(
				"resumeSession",
				"conformance-resumed",
				CONFORMANCE_AGENT_NAME,
				{ cwd: "/workspace" },
			);
			expect(resumed).toMatchObject({
				sessionId: "conformance-resumed",
				mode: "native",
			});
			await backend.call("closeSession", resumed.sessionId);
			await backend.call("destroySession", sessionId);
			expect(await backend.call<any[]>("listSessions")).not.toContainEqual(
				expect.objectContaining({ sessionId }),
			);
			offSession();
			offPermission();
		}, 90_000);

		test("unexpected ACP adapter exits surface through agentCrashed", async () => {
			const crashes: any[] = [];
			const offCrash = backend.on("agentCrashed", (event) =>
				crashes.push(event),
			);
			const sessionId = await backend.call<string>(
				"createSession",
				CONFORMANCE_AGENT_NAME,
				{ skipOsInstructions: true },
			);
			await backend
				.call("sendPrompt", sessionId, "crash-adapter")
				.catch(() => undefined);
			await eventually(
				() => crashes,
				(events) => events.some((event) => event.sessionId === sessionId),
				15_000,
			);
			await backend.call("closeSession", sessionId);
			offCrash();
		}, 30_000);

		if (options.verifyBackend) {
			test("backend-specific integration hooks observe the shared contract", async () => {
				await options.verifyBackend?.(backend);
			});
		}
	});
}
