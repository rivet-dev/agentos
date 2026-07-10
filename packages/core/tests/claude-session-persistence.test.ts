import claude from "@agentos-software/claude-code";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	createAnthropicFixture,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";

function requestContains(request: unknown, expected: string): boolean {
	return JSON.stringify(request).includes(expected);
}

async function createClaudeVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		software: [claude],
	});
}

async function closeSessionAndWait(
	vm: AgentOs,
	sessionId: string,
): Promise<void> {
	vm.closeSession(sessionId);
	const closePromise = (
		vm as AgentOs & { _sessionClosePromises: Map<string, Promise<void>> }
	)._sessionClosePromises.get(sessionId);
	if (closePromise) await closePromise;
}

async function waitForClaudeSession(vm: AgentOs, sessionId: string) {
	const deadline = Date.now() + 5_000;
	while (Date.now() < deadline) {
		const files = await vm.readdirRecursive("/home/agentos/.claude");
		if (files.some((entry) => entry.path.includes(sessionId))) return files;
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	return vm.readdirRecursive("/home/agentos/.claude");
}

describe("Claude native session persistence", () => {
	test("resumes an existing persisted Claude session natively", async () => {
		const firstPrompt = "Remember the native Claude token: maple-1618.";
		const secondPrompt = "What native Claude token did I give you?";
		const result = await startLlmock([
			createAnthropicFixture(
				{ predicate: (req) => requestContains(req, firstPrompt) },
				{ content: "I will remember maple-1618." },
			),
			createAnthropicFixture(
				{ predicate: (req) => requestContains(req, secondPrompt) },
				{ content: "The token was maple-1618." },
			),
		]);
		const vm = await createClaudeVm(result.url);

		let sessionId: string | undefined;
		try {
			const env = {
				ANTHROPIC_API_KEY: "mock-key",
				ANTHROPIC_BASE_URL: result.url,
			};
			sessionId = (
				await vm.createSession("claude", { cwd: "/home/agentos", env })
			).sessionId;
			const persistedSessionId = sessionId;
			expect(vm.getSessionCapabilities(sessionId)?.loadSession).toBe(true);
			expect(
				(await vm.prompt(sessionId, firstPrompt)).response.error,
			).toBeUndefined();
			const persistedFiles = await waitForClaudeSession(vm, sessionId);
			await closeSessionAndWait(vm, sessionId);
			expect(
				persistedFiles.some((entry) => entry.path.includes(persistedSessionId)),
				`Claude did not persist session ${sessionId}; files: ${persistedFiles
					.map((entry) => entry.path)
					.join(", ")}`,
			).toBe(true);

			const resumed = await vm.resumeSession(sessionId, "claude", {
				cwd: "/home/agentos",
				env,
				transcriptPath: `/root/.agentos/threads/${sessionId}.md`,
			});
			expect(resumed).toEqual({ sessionId, mode: "native" });
			expect(
				(await vm.prompt(resumed.sessionId, secondPrompt)).response.error,
			).toBeUndefined();

			const secondRequest = result.mock
				.getRequests()
				.find((request) => requestContains(request, secondPrompt));
			expect(secondRequest).toBeDefined();
			expect(requestContains(secondRequest, firstPrompt)).toBe(true);
			expect(
				requestContains(secondRequest, "You are continuing an earlier session"),
			).toBe(false);
		} finally {
			if (sessionId) vm.closeSession(sessionId);
			await vm.dispose();
			await stopLlmock(result.mock);
		}
	}, 120_000);

	test("restores Claude from the transcript when native state is missing", async () => {
		const result = await startLlmock([
			createAnthropicFixture(
				{ predicate: () => true },
				{ content: "Recovered from the transcript pointer." },
			),
		]);
		const vm = await createClaudeVm(result.url);

		let liveSessionId: string | undefined;
		try {
			const externalSessionId = "00000000-0000-4000-8000-000000000077";
			const transcriptPath = `/root/.agentos/threads/${externalSessionId}.md`;
			const resumed = await vm.resumeSession(externalSessionId, "claude", {
				cwd: "/home/agentos",
				env: {
					ANTHROPIC_API_KEY: "mock-key",
					ANTHROPIC_BASE_URL: result.url,
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
				result.mock
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
			await stopLlmock(result.mock);
		}
	}, 120_000);
});
