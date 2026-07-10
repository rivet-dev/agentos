import { resolve } from "node:path";
import common from "@agentos-software/common";
import piCli from "@agentos-software/pi-cli";
import type { Fixture, ToolCall } from "@copilotkit/llmock";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	createAnthropicFixture,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");

function getRequestBody(req: unknown): Record<string, unknown> {
	const direct = req as Record<string, unknown>;
	const body = direct.body;
	return body && typeof body === "object"
		? (body as Record<string, unknown>)
		: direct;
}

function requestContains(req: unknown, expected: string): boolean {
	return JSON.stringify(getRequestBody(req)).includes(expected);
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
					!JSON.stringify(getRequestBody(req)).includes('"role":"tool"'),
			},
			{ toolCalls: [toolCall] },
		),
		createAnthropicFixture(
			{
				predicate: (req) =>
					JSON.stringify(getRequestBody(req)).includes('"role":"tool"') &&
					JSON.stringify(getRequestBody(req)).includes(expectedToolResult),
			},
			{ content: finalText },
		),
	];
}

async function createPiCliVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
		software: [common, piCli],
	});
}

async function createVmPiHome(vm: AgentOs, mockUrl: string): Promise<string> {
	const homeDir = "/home/agentos";
	await vm.mkdir(`${homeDir}/.pi/agent`, { recursive: true });
	await vm.writeFile(
		`${homeDir}/.pi/agent/models.json`,
		JSON.stringify(
			{
				providers: {
					anthropic: {
						baseUrl: mockUrl,
						apiKey: "mock-key",
					},
				},
			},
			null,
			2,
		),
	);
	return homeDir;
}

async function createVmWorkspace(vm: AgentOs): Promise<string> {
	const workspaceDir = "/home/agentos/workspace";
	await vm.mkdir(workspaceDir, { recursive: true });
	return workspaceDir;
}

describe("full createSession('pi-cli') inside the VM", () => {
	test("runs the unmodified Pi CLI ACP flow end-to-end for write tool calls", async () => {
		const workspacePath = "/home/agentos/workspace/notes.txt";
		const fixtures = createToolFixtures(
			{
				name: "write",
				arguments: JSON.stringify({
					path: workspacePath,
					content: "hello from pi cli write",
				}),
			},
			"Successfully wrote",
			"notes.txt was created successfully.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiCliVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi-cli", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const events: { method: string; params?: unknown }[] = [];
			const unsubscribeEvents = vm.onSessionEvent(sessionId, (event) => {
				events.push(event);
			});
			const { response, text } = await vm.prompt(
				sessionId,
				`Create ${workspacePath} with the text hello from pi cli write.`,
			);
			unsubscribeEvents();

			expect(response.error).toBeUndefined();
			expect(text).toContain("notes.txt was created successfully.");
			expect(new TextDecoder().decode(await vm.readFile(workspacePath))).toBe(
				"hello from pi cli write",
			);
			expect(mock.getRequests().length).toBeGreaterThanOrEqual(2);

			expect(
				events.some(
					(event) =>
						event.method === "session/update" &&
						JSON.stringify(event.params).includes("tool_call"),
				),
			).toBe(true);
			expect(
				events.some(
					(event) =>
						event.method === "session/update" &&
						JSON.stringify(event.params).includes('"completed"'),
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

	test("resumes an existing persisted Pi CLI session natively", async () => {
		const firstPrompt = "Remember the native Pi CLI token: cedar-3141.";
		const secondPrompt = "What native Pi CLI token did I give you?";
		const { mock, url } = await startLlmock([
			createAnthropicFixture(
				{ predicate: (req) => requestContains(req, firstPrompt) },
				{ content: "I will remember cedar-3141." },
			),
			createAnthropicFixture(
				{ predicate: (req) => requestContains(req, secondPrompt) },
				{ content: "The token was cedar-3141." },
			),
		]);
		const vm = await createPiCliVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			const env = {
				HOME: homeDir,
				ANTHROPIC_API_KEY: "mock-key",
				ANTHROPIC_BASE_URL: url,
			};
			sessionId = (await vm.createSession("pi-cli", { cwd: workspaceDir, env }))
				.sessionId;
			expect(vm.getSessionCapabilities(sessionId)?.loadSession).toBe(true);
			expect(
				(await vm.prompt(sessionId, firstPrompt)).response.error,
			).toBeUndefined();
			vm.closeSession(sessionId);

			const resumed = await vm.resumeSession(sessionId, "pi-cli", {
				cwd: workspaceDir,
				env,
				transcriptPath: `/root/.agentos/threads/${sessionId}.md`,
			});
			expect(resumed).toEqual({ sessionId, mode: "native" });
			expect(
				(await vm.prompt(resumed.sessionId, secondPrompt)).response.error,
			).toBeUndefined();
			const secondRequest = mock
				.getRequests()
				.find((request) => requestContains(request, secondPrompt));
			expect(secondRequest).toBeDefined();
			expect(requestContains(secondRequest, firstPrompt)).toBe(true);
		} finally {
			if (sessionId) vm.closeSession(sessionId);
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	test("restores Pi CLI from the transcript when native state is missing", async () => {
		const { mock, url } = await startLlmock([
			createAnthropicFixture(
				{ predicate: () => true },
				{ content: "Recovered from the transcript pointer." },
			),
		]);
		const vm = await createPiCliVm(url);

		let liveSessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			const externalSessionId = "00000000-0000-4000-8000-000000000088";
			const transcriptPath = `/root/.agentos/threads/${externalSessionId}.md`;
			const resumed = await vm.resumeSession(externalSessionId, "pi-cli", {
				cwd: workspaceDir,
				env: {
					HOME: homeDir,
					ANTHROPIC_API_KEY: "mock-key",
					ANTHROPIC_BASE_URL: url,
				},
				transcriptPath,
			});
			liveSessionId = resumed.sessionId;
			expect(resumed.mode).toBe("fallback");

			expect(
				(await vm.prompt(liveSessionId, "Continue the recovered session."))
					.response.error,
			).toBeUndefined();
			expect(
				mock
					.getRequests()
					.some(
						(request) =>
							requestContains(
								request,
								"You are continuing an earlier session",
							) && requestContains(request, transcriptPath),
					),
			).toBe(true);
		} finally {
			if (liveSessionId) vm.closeSession(liveSessionId);
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);

	// Blocked on shell `>` redirect output being visible to `vm.readFile()`.
	// This is the unmodified upstream Pi CLI bash path (`createLocalBashOperations`
	// spawning the shell directly), with no Agent OS operations override, so the
	// failure is a runtime gap independent of the SDK adapter: the redirect runs
	// inside the guest shell but the written bytes do not reconcile to the host
	// read path yet. Tracked in ~/.agents/todo/agentos-runtime-fixes.md
	// (shell-exec redirect visibility).
	test.skip("runs the unmodified Pi CLI ACP flow end-to-end for bash tool calls", async () => {
		const workspacePath = "/home/agentos/workspace/bash-output.txt";
		const fixtures = createToolFixtures(
			{
				name: "bash",
				arguments: JSON.stringify({
					command: `printf 'bash-ok' > ${workspacePath}`,
					timeout: 10,
				}),
			},
			"bash-ok",
			"bash-output.txt was written successfully.",
		);
		const { mock, url } = await startLlmock(fixtures);
		const vm = await createPiCliVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmPiHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = (
				await vm.createSession("pi-cli", {
					cwd: workspaceDir,
					env: {
						HOME: homeDir,
						ANTHROPIC_API_KEY: "mock-key",
						ANTHROPIC_BASE_URL: url,
					},
				})
			).sessionId;

			const { response, text } = await vm.prompt(
				sessionId,
				`Use bash to write bash-ok into ${workspacePath}.`,
			);

			expect(response.error).toBeUndefined();
			expect(text).toContain("bash-output.txt was written successfully.");
			expect(new TextDecoder().decode(await vm.readFile(workspacePath))).toBe(
				"bash-ok",
			);
			expect(mock.getRequests().length).toBeGreaterThanOrEqual(2);
		} finally {
			if (sessionId) {
				vm.closeSession(sessionId);
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);
});
