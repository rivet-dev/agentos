import { resolve } from "node:path";
import opencode from "@agentos-software/opencode";
import { describe, expect, test } from "vitest";
import type { AgentCapabilities, AgentInfo } from "../src/agent-os.js";
import { AgentOs } from "../src/agent-os.js";
import {
	DEFAULT_TEXT_FIXTURE,
	startLlmock,
	stopLlmock,
} from "./helpers/llmock-helper.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import {
	createVmOpenCodeHome,
	createVmWorkspace,
} from "./helpers/opencode-helper.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");

async function createOpenCodeVm(mockUrl: string): Promise<AgentOs> {
	return AgentOs.create({
		loopbackExemptPorts: [Number(new URL(mockUrl).port)],
		mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
		software: [opencode],
	});
}

describe("real createSession('opencode')", () => {
	test("initializes the projected OpenCode ACP package inside the VM", async () => {
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
});
