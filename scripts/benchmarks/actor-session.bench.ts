/**
 * End-to-end Rivet actor agent-session benchmark.
 *
 * Measures two user-visible boundaries on a warmed, filesystem-verified actor:
 *   1. createSession() resolution (the agent process completed its ACP handshake)
 *   2. the first prompt reaching llmock, emitting agent text, and completing
 *
 * A fresh session is used for every turn. Warmup turns are excluded from the
 * summary so package projection, actor boot, and agent snapshot initialization
 * do not distort steady-state session startup.
 *
 * Usage:
 *   pnpm exec tsx scripts/benchmarks/actor-session.bench.ts
 *   pnpm exec tsx scripts/benchmarks/actor-session.bench.ts --iterations=3 --warmup=1
 *   pnpm exec tsx scripts/benchmarks/actor-session.bench.ts --agents=claude,pi,opencode
 */

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { LLMock } from "@copilotkit/llmock";
import { createClient } from "@rivet-dev/agentos/client";

const SENTINEL = "agentos-actor-session-benchmark-ok";
const RIVET_NAMESPACE = "default";
const RIVET_TOKEN = "dev";
const RIVET_POOL_NAME = "default";
const ALL_AGENTS = [
	{
		id: "claude",
		label: "Claude Code",
		env: (mockUrl: string) => ({
			ANTHROPIC_API_KEY: "mock-key",
			ANTHROPIC_BASE_URL: mockUrl,
		}),
	},
	{
		id: "pi",
		label: "Pi",
		env: (mockUrl: string) => ({
			HOME: "/home/agentos/benchmark",
			ANTHROPIC_API_KEY: "mock-key",
			ANTHROPIC_BASE_URL: mockUrl,
			PI_SKIP_VERSION_CHECK: "1",
		}),
	},
	{
		id: "codex",
		label: "Codex",
		env: (mockUrl: string) => ({
			HOME: "/home/agentos/benchmark",
			CODEX_HOME: "/home/agentos/benchmark/.codex",
			OPENAI_API_KEY: "mock-key",
			OPENAI_BASE_URL: `${mockUrl}/v1`,
		}),
	},
	{
		id: "opencode",
		label: "OpenCode",
		env: (mockUrl: string) => ({
			HOME: "/home/agentos/benchmark",
			ANTHROPIC_API_KEY: "mock-key",
			ANTHROPIC_BASE_URL: `${mockUrl}/v1`,
		}),
	},
] as const;

type AgentId = (typeof ALL_AGENTS)[number]["id"];

interface Stats {
	mean: number;
	p50: number;
	p95: number;
	min: number;
	max: number;
}

interface SuccessfulTrial {
	agent: AgentId;
	label: string;
	phase: "warmup" | "measured";
	iteration: number;
	sessionId: string;
	createSessionMs: number;
	promptToProviderRequestMs: number;
	promptToFirstUpdateMs: number | null;
	promptToFirstTextMs: number;
	firstTextSource: "session-event" | "prompt-result";
	promptToSentinelMs: number;
	promptCompleteMs: number;
	sessionCreateToFirstTextMs: number;
	sessionCreateToPromptCompleteMs: number;
	mockPath: string;
	stopReason: unknown;
}

interface FailedTrial {
	agent: AgentId;
	label: string;
	phase: "warmup" | "measured";
	iteration: number;
	failed: true;
	error: string;
	createSessionMs?: number;
}

type Trial = SuccessfulTrial | FailedTrial;

interface ActivePrompt {
	promptStartedAt: number;
	firstUpdateAt: number | null;
	firstTextAt: number | null;
	sentinelAt: number | null;
	updates: string[];
}

const argv = process.argv.slice(2);
const arg = (name: string, fallback: string): string =>
	argv.find((value) => value.startsWith(`--${name}=`))?.split("=")[1] ??
	fallback;
const positiveInteger = (name: string, fallback: number, allowZero = false) => {
	const value = Number.parseInt(arg(name, String(fallback)), 10);
	if (!Number.isInteger(value) || value < (allowZero ? 0 : 1)) {
		throw new Error(`--${name} must be ${allowZero ? "a non-negative" : "a positive"} integer`);
	}
	return value;
};

const iterations = positiveInteger("iterations", 5);
const warmup = positiveInteger("warmup", 1, true);
const settleMs = positiveInteger("settle-ms", 750, true);
const timeoutMs = positiveInteger("timeout-ms", 60_000);
const serverStartAttempts = positiveInteger("server-start-attempts", 3);
const outputPath = arg("output", "");
const requestedAgentIds = new Set(
	arg(
		"agents",
		ALL_AGENTS.map((agent) => agent.id).join(","),
	)
		.split(",")
		.map((value) => value.trim())
		.filter(Boolean),
);
const unknownAgents = [...requestedAgentIds].filter(
	(id) => !ALL_AGENTS.some((agent) => agent.id === id),
);
if (unknownAgents.length > 0) {
	throw new Error(`Unknown --agents values: ${unknownAgents.join(", ")}`);
}
const agents = ALL_AGENTS.filter((agent) => requestedAgentIds.has(agent.id));
if (agents.length === 0) throw new Error("--agents selected no agents");

function round(value: number): number {
	return Math.round(value * 10) / 10;
}

function percentile(sorted: number[], fraction: number): number {
	const index = Math.ceil(sorted.length * fraction) - 1;
	return sorted[Math.max(0, Math.min(sorted.length - 1, index))];
}

function stats(values: number[]): Stats | null {
	if (values.length === 0) return null;
	const sorted = [...values].sort((a, b) => a - b);
	return {
		mean: round(values.reduce((sum, value) => sum + value, 0) / values.length),
		p50: round(percentile(sorted, 0.5)),
		p95: round(percentile(sorted, 0.95)),
		min: round(sorted[0]),
		max: round(sorted[sorted.length - 1]),
	};
}

function elapsed(startedAt: number): number {
	return round(performance.now() - startedAt);
}

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

async function withTimeout<T>(
	promise: Promise<T>,
	ms: number,
	label: string,
): Promise<T> {
	let timer: NodeJS.Timeout | undefined;
	try {
		return await Promise.race([
			promise,
			new Promise<never>((_, reject) => {
				timer = setTimeout(
					() => reject(new Error(`${label} timed out after ${ms} ms`)),
					ms,
				);
			}),
		]);
	} finally {
		if (timer) clearTimeout(timer);
	}
}

async function reservePort(): Promise<number> {
	return new Promise((resolve, reject) => {
		const server = createServer();
		server.once("error", reject);
		server.listen(0, "127.0.0.1", () => {
			const address = server.address();
			if (!address || typeof address === "string") {
				server.close();
				reject(new Error("Failed to reserve a benchmark port"));
				return;
			}
			const port = address.port;
			server.close((error) => (error ? reject(error) : resolve(port)));
		});
	});
}

function pipeServerOutput(child: ChildProcess): void {
	for (const stream of [child.stdout, child.stderr]) {
		stream?.setEncoding("utf8");
		stream?.on("data", (chunk: string) => {
			for (const line of chunk.trimEnd().split("\n")) {
				if (line) console.error(`[actor-server] ${line}`);
			}
		});
	}
}

async function waitForServer(
	child: ChildProcess,
	endpoint: string,
): Promise<void> {
	const startedAt = performance.now();
	while (performance.now() - startedAt < 30_000) {
		if (child.exitCode !== null) {
			throw new Error(`Actor server exited early with code ${child.exitCode}`);
		}
		try {
			await fetch(endpoint, { signal: AbortSignal.timeout(500) });
			return;
		} catch {
			await sleep(50);
		}
	}
	throw new Error(`Actor server did not become ready at ${endpoint}`);
}

async function configureLocalRunner(
	child: ChildProcess,
	endpoint: string,
): Promise<void> {
	const headers = { Authorization: `Bearer ${RIVET_TOKEN}` };
	const datacentersResponse = await fetch(
		`${endpoint}/datacenters?namespace=${RIVET_NAMESPACE}`,
		{ headers },
	);
	if (!datacentersResponse.ok) {
		throw new Error(
			`Failed to list local Rivet datacenters: ${datacentersResponse.status}`,
		);
	}
	const datacenters = (await datacentersResponse.json()) as {
		datacenters: Array<{ name: string }>;
	};
	const datacenter = datacenters.datacenters[0]?.name;
	if (!datacenter) throw new Error("Local Rivet engine returned no datacenters");

	const configResponse = await fetch(
		`${endpoint}/runner-configs/${RIVET_POOL_NAME}?namespace=${RIVET_NAMESPACE}`,
		{
			method: "PUT",
			headers: { ...headers, "Content-Type": "application/json" },
			body: JSON.stringify({ datacenters: { [datacenter]: { normal: {} } } }),
		},
	);
	if (!configResponse.ok) {
		throw new Error(
			`Failed to configure local Rivet runner: ${configResponse.status}`,
		);
	}

	const startedAt = performance.now();
	while (performance.now() - startedAt < 30_000) {
		if (child.exitCode !== null) {
			throw new Error(`Actor server exited early with code ${child.exitCode}`);
		}
		const response = await fetch(
			`${endpoint}/envoys?namespace=${RIVET_NAMESPACE}&name=${RIVET_POOL_NAME}`,
			{ headers },
		);
		if (response.ok) {
			const body = (await response.json()) as { envoys: unknown[] };
			if (body.envoys.length > 0) return;
		}
		await sleep(100);
	}
	throw new Error("Local Rivet runner did not register an envoy");
}

async function retryActorReady<T>(operation: () => Promise<T>): Promise<T> {
	const startedAt = performance.now();
	while (true) {
		try {
			return await operation();
		} catch (error) {
			const code = (error as { code?: unknown } | null)?.code;
			const group = (error as { group?: unknown } | null)?.group;
			const runnerStarting =
				code === "no_runner_config_configured" ||
				(group === "namespace" && code === "not_found") ||
				(group === "core" && code === "internal_error");
			if (
				!runnerStarting ||
				performance.now() - startedAt >= 120_000
			) {
				throw error;
			}
			await sleep(code === "internal_error" ? 1_000 : 100);
		}
	}
}

async function signalBenchmarkProcesses(
	child: ChildProcess,
	storagePath: string,
	signal: NodeJS.Signals,
): Promise<void> {
		if (process.platform !== "win32" && child.pid !== undefined) {
			try {
				process.kill(-child.pid, signal);
			} catch {
				// The server process group may already be gone.
			}
		}
	child.kill(signal);

	// Rivet Engine creates its own process group, so it can outlive the server
	// process even when the server group is signalled. On Linux, identify only
	// engine descendants carrying this run's unique storage marker.
	if (process.platform === "linux") {
		const marker = Buffer.from(`RIVETKIT_STORAGE_PATH=${storagePath}\0`);
		for (const entry of await readdir("/proc", { withFileTypes: true })) {
			if (!entry.isDirectory() || !/^\d+$/u.test(entry.name)) continue;
			try {
				const environ = await readFile(`/proc/${entry.name}/environ`);
				if (environ.indexOf(marker) !== -1) {
					process.kill(Number(entry.name), signal);
				}
			} catch {
				// Processes may exit or become inaccessible while /proc is scanned.
			}
		}
	}
}

async function stopChild(
	child: ChildProcess,
	storagePath: string,
): Promise<void> {

	if (child.exitCode !== null && process.platform === "win32") return;
	await signalBenchmarkProcesses(child, storagePath, "SIGTERM");
	if (child.exitCode === null) {
		await Promise.race([
			new Promise<void>((resolve) => child.once("exit", () => resolve())),
			sleep(2_000),
		]);
	} else {
		await sleep(250);
	}
	await signalBenchmarkProcesses(child, storagePath, "SIGKILL");
}

async function warmActor(actor: any): Promise<number> {
	const startedAt = performance.now();
	await actor.mkdir("/home/agentos/benchmark");
	await actor.writeFile(
		"/home/agentos/benchmark/actor-warm.txt",
		"actor-warm-ok\n",
	);
	const execResult = await actor.exec(
		"echo exec-warm-ok > /home/agentos/benchmark/exec-warm.txt",
	);
	const warmText = new TextDecoder()
		.decode(await actor.readFile("/home/agentos/benchmark/actor-warm.txt"))
		.trim();
	const execText = new TextDecoder()
		.decode(await actor.readFile("/home/agentos/benchmark/exec-warm.txt"))
		.trim();
	if (
		execResult.exitCode !== 0 ||
		warmText !== "actor-warm-ok" ||
		execText !== "exec-warm-ok"
	) {
		throw new Error(
			`Actor filesystem/exec warmup verification failed: ${JSON.stringify({
				execResult,
				warmText,
				execText,
			})}`,
		);
	}
	return elapsed(startedAt);
}

async function configureOpenCode(actor: any, mockUrl: string): Promise<void> {
	await actor.mkdir("/home/agentos/benchmark/.config");
	await actor.mkdir("/home/agentos/benchmark/.config/opencode");
	await actor.writeFile(
		"/home/agentos/benchmark/.config/opencode/opencode.json",
		JSON.stringify({
			autoupdate: false,
			share: "disabled",
			snapshot: false,
			model: "anthropic/claude-sonnet-4-20250514",
			provider: { anthropic: { options: { baseURL: `${mockUrl}/v1` } } },
		}),
	);
}

function sessionIdFrom(result: unknown): string {
	if (typeof result === "string") return result;
	if (result && typeof result === "object") {
		const sessionId = (result as { sessionId?: unknown }).sessionId;
		if (typeof sessionId === "string") return sessionId;
	}
	throw new Error(`createSession returned no session id: ${JSON.stringify(result)}`);
}

function gitMetadata(): { revision: string; dirty: boolean } {
	try {
		return {
			revision: execFileSync(
				"jj",
				["log", "-r", "@", "--no-graph", "-T", "commit_id.short()"],
				{ encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
			).trim(),
			dirty:
				execFileSync("jj", ["diff", "--summary"], {
					encoding: "utf8",
					stdio: ["ignore", "pipe", "ignore"],
				}).trim().length > 0,
		};
	} catch {
		try {
			return {
				revision: execFileSync("git", ["rev-parse", "--short", "HEAD"], {
					encoding: "utf8",
				}).trim(),
				dirty:
					execFileSync("git", ["status", "--porcelain"], {
						encoding: "utf8",
					}).trim().length > 0,
			};
		} catch {
			return { revision: "unknown", dirty: true };
		}
	}
}

async function main(): Promise<void> {
	const benchmarkStartedAt = performance.now();
	const enginePort = await reservePort();
	const mockPort = await reservePort();
	const endpoint = `http://127.0.0.1:${enginePort}`;
	const mockUrl = `http://127.0.0.1:${mockPort}`;
	const storagePath = await mkdtemp(join(tmpdir(), "agentos-actor-session-bench-"));
	const mock = new LLMock({
		port: mockPort,
		host: "127.0.0.1",
		logLevel: "silent",
		latency: 0,
		chunkSize: 1024,
	});
	mock.addFixture({
		match: { predicate: () => true },
		response: { content: SENTINEL },
		streamingProfile: { ttft: 0, tps: 10_000, jitter: 0 },
	});

	let server: ChildProcess | undefined;
	const trials: Trial[] = [];
	const actorWarmups: Array<{
		agent: AgentId;
		actorWarmupMs: number;
		filesystemVerified: true;
	}> = [];

	try {
		await mock.start();
		for (let attempt = 1; attempt <= serverStartAttempts; attempt += 1) {
			server = spawn(
				"pnpm",
				["exec", "tsx", "scripts/benchmarks/actor-session-server.ts"],
				{
					cwd: join(import.meta.dirname, "..", ".."),
					detached: process.platform !== "win32",
					env: {
						...process.env,
						RIVET_TOKEN,
						RIVET_NAMESPACE,
						BENCH_AGENTS: agents.map((agent) => agent.id).join(","),
						RIVET_RUN_ENGINE_PORT: String(enginePort),
						BENCH_MOCK_PORT: String(mockPort),
						RIVETKIT_STORAGE_PATH: storagePath,
						RIVET_EXPOSE_ERRORS: "1",
					},
					stdio: ["ignore", "pipe", "pipe"],
				},
			);
			pipeServerOutput(server);
			try {
				await waitForServer(server, endpoint);
				await configureLocalRunner(server, endpoint);
				break;
			} catch (error) {
				await stopChild(server, storagePath);
				server = undefined;
				if (attempt === serverStartAttempts) throw error;
				console.error(
					`Actor server startup failed (attempt ${attempt}/${serverStartAttempts}); retrying`,
				);
				await sleep(500);
			}
		}

		const client = createClient({
			endpoint,
			token: RIVET_TOKEN,
			namespace: RIVET_NAMESPACE,
			poolName: RIVET_POOL_NAME,
		});
		for (const agent of agents) {
			const actor = client.vm.getOrCreate(
				`actor-session-benchmark-${agent.id}-${process.pid}`,
			);
			let connection: ReturnType<typeof actor.connect> | undefined;
			const activePrompts = new Map<string, ActivePrompt>();

			try {
				const actorWarmupMs = await retryActorReady(() => warmActor(actor));
				actorWarmups.push({
					agent: agent.id,
					actorWarmupMs,
					filesystemVerified: true,
				});
				if (agent.id === "opencode") await configureOpenCode(actor, mockUrl);
				connection = actor.connect();
				connection.on("sessionEvent", (data: any) => {
					const active = activePrompts.get(data.sessionId);
					if (!active || data.event?.method !== "session/update") return;
					const now = performance.now();
					const serialized = JSON.stringify(data.event.params);
					if (active.updates.length < 20) active.updates.push(serialized);
					if (process.env.BENCH_DEBUG_EVENTS === "1") {
						console.error(`[session-event] ${serialized}`);
					}
					if (active.firstUpdateAt === null) active.firstUpdateAt = now;
					if (
						active.firstTextAt === null &&
						serialized.includes("agent_message_chunk")
					) {
						active.firstTextAt = now;
					}
					if (active.sentinelAt === null && serialized.includes(SENTINEL)) {
						active.sentinelAt = now;
					}
				});
				console.error(
					`[${agent.id}] actor warmed and verified in ${actorWarmupMs} ms`,
				);

				for (let index = 0; index < warmup + iterations; index += 1) {
					const phase = index < warmup ? "warmup" : "measured";
					const iteration =
						phase === "warmup" ? index + 1 : index - warmup + 1;
					let sessionId: string | undefined;
					let promptTimedOut = false;
					let createSessionMs: number | undefined;
					const sessionAndPromptStartedAt = performance.now();
					try {
						const createStartedAt = performance.now();
						sessionId = sessionIdFrom(
							await withTimeout(
								actor.createSession(agent.id, {
									cwd: "/home/agentos/benchmark",
									env: agent.env(mockUrl),
								}),
								timeoutMs,
								`${agent.label} createSession`,
							),
						);
						createSessionMs = elapsed(createStartedAt);

						mock.clearRequests();
						const promptStartedAt = performance.now();
						const promptStartedEpochMs = Date.now();
						const active: ActivePrompt = {
							promptStartedAt,
							firstUpdateAt: null,
							firstTextAt: null,
							sentinelAt: null,
							updates: [],
						};
						activePrompts.set(sessionId, active);
						const response = (await withTimeout(
							actor.sendPrompt(sessionId, "Reply with the mock response."),
							timeoutMs,
							`${agent.label} first prompt`,
						)) as { text?: unknown; stopReason?: unknown };
						const promptCompleteAt = performance.now();
						const responseText =
							typeof response.text === "string" ? response.text : "";
						let firstTextSource: SuccessfulTrial["firstTextSource"] =
							"session-event";
						if (active.firstTextAt === null && responseText.includes(SENTINEL)) {
							active.firstTextAt = promptCompleteAt;
							active.sentinelAt = promptCompleteAt;
							firstTextSource = "prompt-result";
						}
						const request = mock.getRequests()[0] as
							| { timestamp: number; path: string }
							| undefined;
						if (!request) throw new Error("Prompt never reached llmock");
						if (active.firstTextAt === null) {
							throw new Error(
								`Prompt emitted no recognized agent text event: ${active.updates.join(" | ")}`,
							);
						}
						if (active.sentinelAt === null) {
							throw new Error("Prompt never emitted the llmock sentinel");
						}

						const trial: SuccessfulTrial = {
							agent: agent.id,
							label: agent.label,
							phase,
							iteration,
							sessionId,
							createSessionMs,
							promptToProviderRequestMs: round(
								request.timestamp - promptStartedEpochMs,
							),
							promptToFirstUpdateMs:
								active.firstUpdateAt === null
									? null
									: round(active.firstUpdateAt - promptStartedAt),
							promptToFirstTextMs: round(
								active.firstTextAt - promptStartedAt,
							),
							firstTextSource,
							promptToSentinelMs: round(active.sentinelAt - promptStartedAt),
							promptCompleteMs: round(promptCompleteAt - promptStartedAt),
							sessionCreateToFirstTextMs: round(
								active.firstTextAt - sessionAndPromptStartedAt,
							),
							sessionCreateToPromptCompleteMs: round(
								promptCompleteAt - sessionAndPromptStartedAt,
							),
							mockPath: request.path,
							stopReason: response.stopReason ?? null,
						};
						trials.push(trial);
						console.error(
							`[${agent.id}] ${phase} ${iteration}: create=${trial.createSessionMs} ms provider=${trial.promptToProviderRequestMs} ms firstText=${trial.promptToFirstTextMs} ms complete=${trial.promptCompleteMs} ms`,
						);
					} catch (error) {
						const message = error instanceof Error ? error.message : String(error);
						promptTimedOut = message.includes("timed out after");
						trials.push({
							agent: agent.id,
							label: agent.label,
							phase,
							iteration,
							failed: true,
							error: message,
							...(createSessionMs === undefined ? {} : { createSessionMs }),
						});
						console.error(`[${agent.id}] ${phase} ${iteration}: FAILED: ${message}`);
					} finally {
						if (sessionId && !promptTimedOut) {
							activePrompts.delete(sessionId);
							await actor.closeSession(sessionId).catch(() => undefined);
							if (settleMs > 0) await sleep(settleMs);
						}
					}
				}
			} finally {
				await connection?.dispose();
			}
		}
	} finally {
		if (server) await stopChild(server, storagePath);
		await mock.stop().catch(() => undefined);
		await rm(storagePath, { recursive: true, force: true });
	}

	const measured = trials.filter(
		(trial): trial is SuccessfulTrial =>
			!("failed" in trial) && trial.phase === "measured",
	);
	const metric = (agent: AgentId, key: keyof SuccessfulTrial) =>
		stats(
			measured
				.filter((trial) => trial.agent === agent)
				.map((trial) => trial[key])
				.filter((value): value is number => typeof value === "number"),
		);
	const sessionMetric = (agent: AgentId) =>
		stats(
			trials
				.filter((trial) => trial.agent === agent && trial.phase === "measured")
				.map((trial) => trial.createSessionMs)
				.filter((value): value is number => typeof value === "number"),
		);
	const summaries = agents.map((agent) => ({
		agent: agent.id,
		label: agent.label,
		successfulSessionIterations: trials.filter(
			(trial) =>
				trial.agent === agent.id &&
				trial.phase === "measured" &&
				typeof trial.createSessionMs === "number",
		).length,
		successfulPromptIterations: measured.filter(
			(trial) => trial.agent === agent.id,
		).length,
		sessionCreate: sessionMetric(agent.id),
		promptToProviderRequest: metric(agent.id, "promptToProviderRequestMs"),
		promptToFirstText: metric(agent.id, "promptToFirstTextMs"),
		promptComplete: metric(agent.id, "promptCompleteMs"),
		sessionCreateToFirstText: metric(agent.id, "sessionCreateToFirstTextMs"),
		sessionCreateToPromptComplete: metric(
			agent.id,
			"sessionCreateToPromptCompleteMs",
		),
	}));

	const output = `${JSON.stringify(
		{
				benchmark: "actor-session-startup-and-first-message",
				createdAt: new Date().toISOString(),
				git: gitMetadata(),
				iterations,
				warmup,
				agents: agents.map((agent) => agent.id),
				mock: {
					provider: "llmock",
					response: SENTINEL,
					artificialLatencyMs: 0,
					streamingTtftMs: 0,
					streamingTokensPerSecond: 10_000,
				},
				methodology: {
					actorWarmup:
						"mkdir + write/read + exec-generated file readback before session trials",
					sessionCreate:
						"client wall time for createSession(), ending after the agent ACP handshake",
					firstMessage:
						"fresh session per trial; timer starts immediately before sendPrompt()",
					firstText:
						"first session/update containing agent_message_chunk; when an adapter emits no streaming update, the text-bearing sendPrompt result is the observable fallback",
					providerRequest:
						"llmock journal timestamp when the provider request was accepted",
					promptComplete: "sendPrompt() promise resolution",
					interTrialSettleMs: settleMs,
				},
				actorWarmups,
				summaries,
				trials,
				totalBenchmarkMs: elapsed(benchmarkStartedAt),
		},
		null,
		2,
	)}\n`;
	if (outputPath) {
		await writeFile(outputPath, output);
		console.error(`Results: ${outputPath}`);
	} else {
		process.stdout.write(output);
	}

	if (trials.some((trial) => "failed" in trial)) process.exitCode = 1;
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
