import {
	createServer,
	type IncomingMessage,
	type ServerResponse,
} from "node:http";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import type { Fixture, ToolCall } from "@copilotkit/llmock";
import opencode from "@agentos-software/opencode";
import { describe, expect, test } from "vitest";
import type { AgentCapabilities, AgentInfo } from "../src/agent-os.js";
import { AgentOs } from "../src/agent-os.js";
import {
	createAnthropicFixture,
	DEFAULT_TEXT_FIXTURE,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";
import {
	createVmOpenCodeHome,
	createVmWorkspace,
	readVmText,
} from "./helpers/opencode-helper.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const REGISTRY_COMMAND_DIR_CANDIDATES = [
	resolve(
		import.meta.dirname,
		"../../../registry/native/target/wasm32-wasip1/release/commands",
	),
	resolve(
		import.meta.dirname,
		"../../../../secure-exec/registry/native/target/wasm32-wasip1/release/commands",
	),
];

function findShellCommandDir(): string | null {
	for (const candidate of REGISTRY_COMMAND_DIR_CANDIDATES) {
		if (
			existsSync(candidate) &&
			existsSync(resolve(candidate, "sh")) &&
			existsSync(resolve(candidate, "bash"))
		) {
			return candidate;
		}
	}
	return null;
}

const shellCommandDir = findShellCommandDir();
const shellSoftware = shellCommandDir
	? [
			{
				commandDir: shellCommandDir,
				commands: [
					{ name: "sh", permissionTier: "full" },
					{ name: "bash", permissionTier: "full", aliasOf: "sh" },
				],
			},
		]
	: [];
const testWithShell = shellCommandDir ? test : test.skip;

type LlmockMessage = {
	role?: string;
	content?: string | null;
};

type ChatCompletionsRequestBody = Record<string, unknown>;

type ChatCompletionsFixture = {
	name: string;
	predicate: (body: ChatCompletionsRequestBody) => boolean;
	response: Record<string, unknown>;
	delayMs?: number;
};

function getLlmockMessages(req: unknown): LlmockMessage[] {
	const directMessages = (req as { messages?: LlmockMessage[] }).messages;
	if (Array.isArray(directMessages)) {
		return directMessages;
	}

	const bodyMessages = (req as { body?: { messages?: LlmockMessage[] } }).body
		?.messages;
	return Array.isArray(bodyMessages) ? bodyMessages : [];
}

function hasToolResultContaining(req: unknown, expected: string): boolean {
	return getLlmockMessages(req).some(
		(message) =>
			message.role === "tool" &&
			typeof message.content === "string" &&
			message.content.includes(expected),
	);
}

function hasAnyToolResult(req: unknown): boolean {
	return getLlmockMessages(req).some((message) => message.role === "tool");
}

function hasUserMessageContaining(req: unknown, expected: string): boolean {
	return getLlmockMessages(req).some(
		(message) =>
			message.role === "user" &&
			typeof message.content === "string" &&
			message.content.includes(expected),
	);
}

function createToolFixtures(
	toolCall: ToolCall,
	expectedToolResult: string,
	finalText: string,
): Fixture[] {
	return [
		createAnthropicFixture(
			{
				predicate: (req) =>
					!getLlmockMessages(req).some((message) => message.role === "tool"),
			},
			{ toolCalls: [toolCall] },
		),
		createAnthropicFixture(
			{
				predicate: (req) => hasToolResultContaining(req, expectedToolResult),
			},
			{ content: finalText },
		),
	];
}

async function readJsonBody(
	req: IncomingMessage,
): Promise<ChatCompletionsRequestBody> {
	const chunks: Buffer[] = [];
	for await (const chunk of req) {
		chunks.push(Buffer.from(chunk));
	}

	return JSON.parse(
		Buffer.concat(chunks).toString("utf8"),
	) as ChatCompletionsRequestBody;
}

function writeJson(
	res: ServerResponse,
	statusCode: number,
	body: Record<string, unknown>,
): void {
	const payload = JSON.stringify(body);
	res.statusCode = statusCode;
	res.setHeader("content-type", "application/json");
	res.setHeader("content-length", Buffer.byteLength(payload));
	res.end(payload);
}

function createChatCompletionResponse(model: string, content: string) {
	return {
		id: `chatcmpl-${model}`,
		object: "chat.completion",
		created: 1,
		model,
		choices: [
			{
				index: 0,
				message: {
					role: "assistant",
					content,
				},
				finish_reason: "stop",
			},
		],
	};
}

async function startChatCompletionsMock(
	fixtures: ChatCompletionsFixture[],
): Promise<{
	url: string;
	requests: ChatCompletionsRequestBody[];
	stop: () => Promise<void>;
}> {
	const requests: ChatCompletionsRequestBody[] = [];
	const server = createServer(async (req, res) => {
		if (req.method !== "POST" || req.url !== "/chat/completions") {
			writeJson(res, 404, { error: "not_found" });
			return;
		}

		try {
			const body = await readJsonBody(req);
			requests.push(body);

			const fixture = fixtures.find((candidate) => candidate.predicate(body));
			if (!fixture) {
				writeJson(res, 500, {
					error: "no_matching_fixture",
					request: body,
				});
				return;
			}

			if (fixture.delayMs) {
				await new Promise((resolve) => setTimeout(resolve, fixture.delayMs));
			}

			writeJson(res, 200, fixture.response);
		} catch (error) {
			writeJson(res, 500, {
				error: "invalid_request",
				message: error instanceof Error ? error.message : String(error),
			});
		}
	});

	await new Promise<void>((resolve) => {
		server.listen(0, "127.0.0.1", () => resolve());
	});
	server.unref();

	const address = server.address();
	if (!address || typeof address === "string") {
		throw new Error("chat completions mock did not expose a TCP port");
	}

	return {
		url: `http://127.0.0.1:${address.port}`,
		requests,
		stop: async () => {
			await new Promise<void>((resolve, reject) => {
				server.close((error) => {
					if (error) reject(error);
					else resolve();
				});
			});
		},
	};
}

async function createOpenCodeVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
		software: [opencode, ...shellSoftware],
	});
}

async function createOpenCodeOnlyVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		software: [opencode],
	});
}

describe("OpenCode session API integration", () => {
	test("full createSession'opencode' inside the VM", async () => {
		const { mock, url } = await startLlmock([DEFAULT_TEXT_FIXTURE]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			const agentInfo = vm.getSessionAgentInfo(sessionId) as AgentInfo;
			expect(agentInfo.name).toBe("OpenCode");
			expect(agentInfo.version).toBeTruthy();

			const capabilities = vm.getSessionCapabilities(
				sessionId,
			) as AgentCapabilities;
			expect(capabilities.promptCapabilities).toMatchObject({
				embeddedContext: true,
				image: true,
			});

			const modes = vm.getSessionModes(sessionId);
			expect(modes?.currentModeId).toBe("build");
			expect(modes?.availableModes.map((mode) => mode.id)).toEqual(
				expect.arrayContaining(["build", "plan"]),
			);

			const configOptions = vm.getSessionConfigOptions(sessionId);
			expect(configOptions.some((option) => option.category === "model")).toBe(
				true,
			);

			expect(vm.listSessions()).toContainEqual({
				sessionId,
				agentType: "opencode",
			});
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("runs the real OpenCode ACP flow end-to-end for write tool calls", async () => {
		const fixtures = createToolFixtures(
			{
				name: "write",
				arguments: JSON.stringify({
					filePath: "notes.txt",
					content: "hello from tool",
				}),
			},
			"hello from tool",
			"notes.txt was created successfully.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			const agentInfo = vm.getSessionAgentInfo(sessionId) as AgentInfo;
			expect(agentInfo.name).toBe("OpenCode");
			expect(agentInfo.version).toBeTruthy();

			const capabilities = vm.getSessionCapabilities(
				sessionId,
			) as AgentCapabilities;
			expect(capabilities.promptCapabilities).toMatchObject({
				embeddedContext: true,
				image: true,
			});

			const modes = vm.getSessionModes(sessionId);
			expect(modes?.currentModeId).toBe("build");
			expect(modes?.availableModes.map((mode) => mode.id)).toEqual(
				expect.arrayContaining(["build", "plan"]),
			);

			const configOptions = vm.getSessionConfigOptions(sessionId);
			expect(configOptions.some((option) => option.category === "model")).toBe(
				true,
			);

			const events: { method: string; params?: unknown }[] = [];
			const unsubscribeEvents = vm.onSessionEvent(sessionId, (event) => {
				events.push(event);
			});
			const { response } = await vm.prompt(
				sessionId,
				"Create notes.txt with the text hello from tool.",
			);
			unsubscribeEvents();

			expect(response.error).toBeUndefined();
			expect(await readVmText(vm, `${workspaceDir}/notes.txt`)).toBe(
				"hello from tool",
			);
			expect(mock.getRequests().length).toBeGreaterThanOrEqual(2);

			expect(
				events.some(
					(event) =>
						event.method === "session/update" &&
						JSON.stringify(event.params).includes("tool_call"),
				),
			).toBe(true);
			expect(events.length).toBeGreaterThan(0);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("supports real OpenCode ACP prompts through Groq and Mistral providers", async () => {
		const providerCases = [
			{
				providerId: "groq",
				modelId: "llama-3.3-70b-versatile",
				envName: "GROQ_API_KEY",
				reply: "groq provider ok",
			},
			{
				providerId: "mistral",
				modelId: "mistral-small-latest",
				envName: "MISTRAL_API_KEY",
				reply: "mistral provider ok",
			},
		] as const;
		const mock = await startChatCompletionsMock(
			providerCases.map((providerCase) => ({
				name: providerCase.providerId,
				predicate: (body) => body.model === providerCase.modelId,
				response: createChatCompletionResponse(
					providerCase.modelId,
					providerCase.reply,
				),
			})),
		);

		try {
			for (const providerCase of providerCases) {
				const vm = await createOpenCodeVm(mock.url);
				let sessionId: string | undefined;
				try {
					const homeDir = await createVmOpenCodeHome(vm, mock.url, {
						model: `${providerCase.providerId}/${providerCase.modelId}`,
						providers: {
							[providerCase.providerId]: {
								options: {
									baseURL: mock.url,
								},
							},
						},
					});
					const workspaceDir = await createVmWorkspace(vm);
					sessionId = (
						await vm.createSession("opencode", {
							cwd: workspaceDir,
							env: {
								HOME: homeDir,
								[providerCase.envName]: "mock-key",
							},
						})
					).sessionId;

					const { response } = await vm.prompt(
						sessionId,
						`Reply with exactly ${providerCase.reply}.`,
					);

					expect(response.error).toBeUndefined();
				} finally {
					if (sessionId) {
						vm.closeSession(sessionId);
					}
					await vm.dispose();
				}
			}

			expect(mock.requests.map((request) => request.model)).toEqual(
				expect.arrayContaining(
					providerCases.map((providerCase) => providerCase.modelId),
				),
			);
		} finally {
			await mock.stop();
		}
	}, 120_000);

	test("integrates OpenCode session metadata, plan mode, and lifecycle into the Agent OS session API", async () => {
		const { mock, url } = await startLlmock([DEFAULT_TEXT_FIXTURE]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			expect(vm.listSessions()).toContainEqual({
				sessionId,
				agentType: "opencode",
			});

			const modelOption = vm
				.getSessionConfigOptions(sessionId)
				.find((option) => option.category === "model");
			expect(modelOption).toMatchObject({
				id: "model",
				category: "model",
				currentValue: "anthropic/claude-sonnet-4-20250514",
				readOnly: true,
			});
			expect(modelOption?.description).toContain("before createSession()");

			const setModelResponse = await vm.setSessionModel(
				sessionId,
				"anthropic/claude-opus-4-1-20250805",
			);
			expect(setModelResponse.error?.message).toContain(
				"configured before createSession()",
			);

			const setModeResponse = await vm.setSessionMode(sessionId, "plan");
			expect(setModeResponse.error).toBeUndefined();
			expect(vm.getSessionModes(sessionId)?.currentModeId).toBe("plan");

			const { response: promptResponse } = await vm.prompt(
				sessionId,
				"Plan the next step without running tools.",
			);
			expect(promptResponse.error).toBeUndefined();
			expect(
				mock
					.getRequests()
					.some((request) =>
						hasUserMessageContaining(request, "Plan Mode - System Reminder"),
					),
			).toBe(true);

			const modelsUsed = mock
				.getRequests()
				.map((request) =>
					request.body && typeof request.body === "object"
						? (request.body as { model?: unknown }).model
						: undefined,
				)
				.filter((model): model is string => typeof model === "string");
			expect(modelsUsed).toContain("claude-sonnet-4-20250514");
			expect(modelsUsed).not.toContain("claude-opus-4-1-20250805");

			const destroyedSessionId = sessionId;
			await vm.destroySession(destroyedSessionId);
			sessionId = undefined;
			expect(vm.listSessions()).not.toContainEqual({
				sessionId: destroyedSessionId,
				agentType: "opencode",
			});
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("real OpenCode missing session/load degrades to transcript fallback", async () => {
		const { mock, url } = await startLlmock([DEFAULT_TEXT_FIXTURE]);
		const traceDir = mkdtempSync(join(tmpdir(), "agentos-opencode-trace-"));
		const tracePath = join(traceDir, "acp.jsonl");
		const previousTracePath = process.env.AGENT_OS_ACP_TRACE_PATH;
		process.env.AGENT_OS_ACP_TRACE_PATH = tracePath;

		let vm: AgentOs | undefined;
		let liveSessionId: string | undefined;
		try {
			vm = await createOpenCodeOnlyVm(url);
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			const externalSessionId = "missing-opencode-session";
			const transcriptPath = `/root/.agentos/threads/${externalSessionId}.md`;

			const resumed = await vm.resumeSession(externalSessionId, "opencode", {
				cwd: workspaceDir,
				env: {
					HOME: homeDir,
					ANTHROPIC_API_KEY: "mock-key",
				},
				transcriptPath,
			});
			liveSessionId = resumed.sessionId;

			expect(resumed.mode).toBe("fallback");
			expect(resumed.sessionId).not.toBe(externalSessionId);

			const { response } = await vm.prompt(
				liveSessionId,
				"Continue from the missing native session.",
			);
			expect(response.error).toBeUndefined();

			expect(
				mock
					.getRequests()
					.some((request) =>
						hasUserMessageContaining(
							request,
							"You are continuing an earlier session",
						),
					),
			).toBe(true);
			expect(
				mock
					.getRequests()
					.some((request) => hasUserMessageContaining(request, transcriptPath)),
			).toBe(true);

			const traces = readFileSync(tracePath, "utf8")
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line) as Record<string, unknown>);
			const loadTrace = traces.find((trace) => trace.method === "session/load");
			expect(loadTrace?.response).toMatchObject({
				error: {
					code: -32603,
					data: {
						details: "NotFoundError",
					},
				},
			});
			expect(
				(loadTrace?.response as { error?: { data?: { kind?: unknown } } }).error
					?.data?.kind,
			).toBeUndefined();
		} finally {
			if (liveSessionId) {
				vm?.closeSession(liveSessionId);
			}
			if (vm) {
				await vm.dispose();
			}
			await stopLlmock(mock);
			if (previousTracePath === undefined) {
				delete process.env.AGENT_OS_ACP_TRACE_PATH;
			} else {
				process.env.AGENT_OS_ACP_TRACE_PATH = previousTracePath;
			}
			rmSync(traceDir, { recursive: true, force: true });
		}
	}, 120_000);

	test("real OpenCode session/load resumes an existing native session", async () => {
		const firstPrompt = "Remember the native resume token: orchid-2718.";
		const secondPrompt = "What native resume token did I give you earlier?";
		const { mock, url } = await startLlmock([
			createAnthropicFixture(
				{
					predicate: (req) => hasUserMessageContaining(req, firstPrompt),
				},
				{ content: "I will remember orchid-2718." },
			),
			createAnthropicFixture(
				{
					predicate: (req) => hasUserMessageContaining(req, secondPrompt),
				},
				{ content: "The token was orchid-2718." },
			),
		]);
		const vm = await createOpenCodeOnlyVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			const firstResponse = await vm.prompt(sessionId, firstPrompt);
			expect(firstResponse.response.error).toBeUndefined();
			vm.closeSession(sessionId);

			const resumed = await vm.resumeSession(sessionId, "opencode", {
				cwd: workspaceDir,
				env: {
					HOME: homeDir,
					ANTHROPIC_API_KEY: "mock-key",
				},
				transcriptPath: `/root/.agentos/threads/${sessionId}.md`,
			});

			expect(resumed).toMatchObject({
				mode: "native",
				sessionId,
			});

			const secondResponse = await vm.prompt(resumed.sessionId, secondPrompt);
			expect(secondResponse.response.error).toBeUndefined();

			const secondRequest = mock
				.getRequests()
				.find((request) => hasUserMessageContaining(request, secondPrompt));
			expect(secondRequest).toBeDefined();
			expect(hasUserMessageContaining(secondRequest, firstPrompt)).toBe(true);
			expect(
				hasUserMessageContaining(
					secondRequest,
					"You are continuing an earlier session",
				),
			).toBe(false);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("surfaces OpenCode cancelSession() honestly through the Agent OS session API", async () => {
		const { mock, url } = await startLlmock([
			{
				match: { predicate: () => true },
				response: {
					content: "This response should outlive the cancel request.",
				},
				latency: 1_500,
			},
		]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			const promptPromise = vm.prompt(
				sessionId,
				"Take a while and then answer.",
			);
			await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));

			const cancelResponse = await vm.cancelSession(sessionId);
			expect(cancelResponse.error).toBeUndefined();
			expect(
				cancelResponse.result as {
					cancelled: boolean;
					requested: boolean;
					via: string;
				},
			).toMatchObject({
				cancelled: true,
				requested: true,
				via: "prompt-fallback",
			});

			const promptResponse = await promptPromise;
			expect(promptResponse.response.error).toBeUndefined();
			expect(
				(promptResponse.response.result as { stopReason?: string } | undefined)
					?.stopReason,
			).toBe("cancelled");
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	testWithShell(
		"supports real OpenCode permission approval through the Agent OS session API",
		async () => {
			const fixtures = [
				createAnthropicFixture(
					{
						predicate: (req) => !hasAnyToolResult(req),
					},
					{
						toolCalls: [
							{
								name: "bash",
								arguments: JSON.stringify({
									command: "echo perm-ok > perm-output.txt",
									description: "write perm-ok",
								}),
							},
						],
					},
				),
				createAnthropicFixture(
					{
						predicate: (req) => hasAnyToolResult(req),
					},
					{ content: "perm-output.txt was written after approval." },
				),
				createAnthropicFixture(
					{
						predicate: (req) =>
							hasUserMessageContaining(
								req,
								"Generate a title for this conversation:",
							),
					},
					{ content: "Permission approval check" },
				),
			];
			const { mock, url } = await startLlmock(fixtures);
			const vm = await createOpenCodeVm(url);

			let sessionId: string | undefined;
			const permissionIds: string[] = [];
			const permissionParams: Record<string, unknown>[] = [];
			const permissionResponses: Promise<unknown>[] = [];
			try {
				const homeDir = await createVmOpenCodeHome(vm, url, {
					permission: { bash: "ask" },
				});
				const workspaceDir = await createVmWorkspace(vm);
				sessionId = (
					await vm.createSession("opencode", {
						cwd: workspaceDir,
						env: {
							HOME: homeDir,
							ANTHROPIC_API_KEY: "mock-key",
						},
					})
				).sessionId;

				vm.onPermissionRequest(sessionId, (request) => {
					permissionIds.push(request.permissionId);
					permissionParams.push(request.params);
					permissionResponses.push(
						vm.respondPermission(sessionId!, request.permissionId, "once"),
					);
				});

				const { response } = await vm.prompt(
					sessionId,
					"Use bash to write perm-ok into perm-output.txt.",
				);
				expect(response.error).toBeUndefined();
				expect(permissionIds).toHaveLength(1);
				expect(
					(permissionParams[0]?._acpMethod as string | undefined) ?? "",
				).toBe("session/request_permission");
				expect(
					(
						permissionParams[0]?.options as
							| Array<{ optionId?: string }>
							| undefined
					)?.map((option) => option.optionId),
				).toEqual(["once", "always", "reject"]);
				await expect(Promise.all(permissionResponses)).resolves.toEqual([
					expect.objectContaining({
						result: expect.objectContaining({ via: "sidecar-request" }),
					}),
				]);
				expect(await readVmText(vm, `${workspaceDir}/perm-output.txt`)).toBe(
					"perm-ok\n",
				);
			} finally {
				if (sessionId) {
					vm.closeSession(sessionId);
				}
				await vm.dispose();
				await stopLlmock(mock);
			}
		},
		120_000,
	);

	test("supports real OpenCode permission rejection through the Agent OS session API", async () => {
		const toolCall = {
			name: "bash",
			arguments: JSON.stringify({
				command: "printf 'perm-no' > perm-output.txt",
				description: "write perm-no",
			}),
		};
		const { mock, url } = await startLlmock([
			createAnthropicFixture(
				{
					predicate: (req) =>
						hasUserMessageContaining(
							req,
							"Use bash to write perm-no into perm-output.txt.",
						),
				},
				{ toolCalls: [toolCall] },
			),
			createAnthropicFixture(
				{
					predicate: (req) =>
						hasAnyToolResult(req) &&
						!hasUserMessageContaining(
							req,
							"Generate a title for this conversation:",
						),
				},
				{ content: "Permission rejected. I did not run the bash command." },
			),
			createAnthropicFixture(
				{
					predicate: (req) =>
						hasUserMessageContaining(
							req,
							"Generate a title for this conversation:",
						),
				},
				{ content: "Permission rejection check" },
			),
		]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		const permissionIds: string[] = [];
		try {
			const homeDir = await createVmOpenCodeHome(vm, url, {
				permission: { bash: "ask" },
			});
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			vm.onPermissionRequest(sessionId, (request) => {
				permissionIds.push(request.permissionId);
				void vm.respondPermission(sessionId!, request.permissionId, "reject");
			});

			const { response } = await vm.prompt(
				sessionId,
				"Use bash to write perm-no into perm-output.txt.",
			);
			expect(response.error).toBeUndefined();
			expect(permissionIds).toHaveLength(1);
			await expect(
				vm.readFile(`${workspaceDir}/perm-output.txt`),
			).rejects.toThrow();
			expect(
				mock
					.getRequests()
					.some((request) =>
						hasUserMessageContaining(
							request,
							"Use bash to write perm-no into perm-output.txt.",
						),
					),
			).toBe(true);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("supports rawSend() mode changes through the Agent OS session API", async () => {
		const { mock, url } = await startLlmock([DEFAULT_TEXT_FIXTURE]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("opencode", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
					},
				})
			).sessionId;

			const receivedEvents: string[] = [];
			const unsubscribe = vm.onSessionEvent(sessionId, (event) => {
				if (event.method !== "session/update") {
					return;
				}
				const serialized = JSON.stringify(event.params);
				if (serialized.includes("current_mode_update")) {
					receivedEvents.push(serialized);
				}
			});

			const setPlanResponse = await vm.setSessionMode(sessionId, "plan");
			expect(setPlanResponse.error).toBeUndefined();
			expect(vm.getSessionModes(sessionId)?.currentModeId).toBe("plan");

			const planPrompt = "Plan once and do not run tools.";
			const { response: planPromptResponse } = await vm.prompt(
				sessionId,
				planPrompt,
			);
			expect(planPromptResponse.error).toBeUndefined();

			const rawBuildResponse = await vm.rawSend(sessionId, "session/set_mode", {
				modeId: "build",
			});
			expect(rawBuildResponse.error).toBeUndefined();
			expect(vm.getSessionModes(sessionId)?.currentModeId).toBe("build");

			const buildPrompt = "Answer normally after returning to build mode.";
			const { response: buildPromptResponse } = await vm.prompt(
				sessionId,
				buildPrompt,
			);
			expect(buildPromptResponse.error).toBeUndefined();
			await new Promise<void>((resolve) => queueMicrotask(resolve));

			expect(
				receivedEvents.some((event) =>
					event.includes('"currentModeId":"plan"'),
				),
			).toBe(true);
			expect(
				receivedEvents.some((event) =>
					event.includes('"currentModeId":"build"'),
				),
			).toBe(true);
			unsubscribe();

			const planRequest = mock
				.getRequests()
				.find((request) => hasUserMessageContaining(request, planPrompt));
			expect(planRequest).toBeDefined();
			expect(
				hasUserMessageContaining(planRequest, "Plan Mode - System Reminder"),
			).toBe(true);

			const buildRequest = mock
				.getRequests()
				.find((request) => hasUserMessageContaining(request, buildPrompt));
			expect(buildRequest).toBeDefined();
			expect(
				hasUserMessageContaining(buildRequest, "Plan Mode - System Reminder"),
			).toBe(false);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);
});
