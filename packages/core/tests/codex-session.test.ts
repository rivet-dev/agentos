import codex from "@agentos-software/codex";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { startResponsesMock } from "./helpers/openai-responses-mock.js";

describe("Codex agent availability", () => {
	const cleanups = new Set<() => Promise<void>>();

	afterEach(async () => {
		for (const stop of cleanups) {
			await stop();
		}
		cleanups.clear();
	});

	test("codex package registers and starts its ACP agent", async () => {
		const vm = await AgentOs.create({
			software: [codex],
		});
		cleanups.add(async () => {
			await vm.dispose();
		});

		expect((await vm.listAgents()).some((agent) => agent.id === "codex")).toBe(
			true,
		);
		const session = await vm.createSession("codex", { cwd: "/root" });
		expect(session.sessionId).toBeTruthy();
		await vm.closeSession(session.sessionId);
	});

	test("packed ACP adapter completes its first mock-backed prompt", async () => {
		const mock = await startResponsesMock([
			{
				name: "final-text",
				predicate: () => true,
				response: {
					id: "resp_codex_acp",
					output: [
						{
							type: "message",
							role: "assistant",
							content: [{ type: "output_text", text: "hello from codex acp" }],
						},
					],
				},
			},
		]);
		cleanups.add(mock.stop);
		const vm = await AgentOs.create({
			loopbackExemptPorts: [mock.port],
			software: [codex],
			limits: {
				jsRuntime: { importCacheMaterializeTimeoutMs: 120_000 },
				wasm: { prewarmTimeoutMs: 120_000 },
			},
		});
		cleanups.add(async () => {
			await vm.dispose();
		});
		const session = await vm.createSession("codex", {
			cwd: "/root",
			env: {
				HOME: "/root",
				CODEX_HOME: "/root/.codex",
				OPENAI_API_KEY: "mock-key",
				OPENAI_BASE_URL: `${mock.url}/v1`,
			},
		});
		const result = await vm.prompt(session.sessionId, "say hello");
		expect(result.text).toContain("hello from codex acp");
		expect(mock.requests.length).toBeGreaterThan(0);
		await vm.closeSession(session.sessionId);
	}, 150_000);
});
