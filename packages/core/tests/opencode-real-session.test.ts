import { resolve } from "node:path";
import opencode from "@agentos-software/opencode";
import { describe, expect, test } from "vitest";
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

describe("real openSession({ agent: 'opencode' })", () => {
	test("initializes the projected OpenCode ACP package inside the VM", async () => {
		const { mock, url } = await startLlmock([DEFAULT_TEXT_FIXTURE]);
		const vm = await createOpenCodeVm(url);

		let sessionId: string | undefined;
		try {
			const homeDir = await createVmOpenCodeHome(vm, url);
			const workspaceDir = await createVmWorkspace(vm);
			sessionId = "main";
			await vm.openSession({
				sessionId,
				agent: "opencode",
				cwd: workspaceDir,
				env: {
					HOME: homeDir,
					ANTHROPIC_API_KEY: "mock-key",
				},
			});

			const agentInfo = await vm.getSessionAgentInfo({ sessionId });
			expect(agentInfo.name).toBe("OpenCode");
			expect(agentInfo.version).toBeTruthy();

			const capabilities = await vm.getSessionCapabilities({ sessionId });
			expect(capabilities.prompt).toMatchObject({
				embeddedContext: true,
				image: true,
			});

			const config = await vm.getSessionConfig({ sessionId });
			// OpenCode currently advertises legacy ACP `modes`, not native
			// `configOptions`; AgentOS deliberately does not invent a mapping.
			expect(config.options.some((option) => option.id === "mode")).toBe(false);

			expect((await vm.listSessions()).sessions).toContainEqual(
				expect.objectContaining({ sessionId, agent: "opencode" }),
			);
		} finally {
			if (sessionId) {
				await vm.unloadSession({ sessionId });
			}
			await vm.dispose();
			await stopLlmock(mock);
		}
	}, 120_000);
});
