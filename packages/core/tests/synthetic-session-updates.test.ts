import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { createProjectedAgentPackage } from "./helpers/projected-agent-package.js";

const MOCK_ACP_ADAPTER = `
let buffer = "";

const sessionState = {
  modeId: "default",
  configOptions: [
    {
      id: "model",
      category: "model",
      label: "Model",
      currentValue: "gpt-5-codex",
    },
    {
      id: "thought_level",
      category: "thought_level",
      label: "Thought Level",
      currentValue: "medium",
    },
  ],
};

function writeResponse(id, result) {
  process.stdout.write(JSON.stringify({
    jsonrpc: "2.0",
    id,
    result,
  }) + "\\n");
}

function writeError(id, message, data) {
  process.stdout.write(JSON.stringify({
    jsonrpc: "2.0",
    id,
    error: {
      code: -32602,
      message,
      ...(data ? { data } : {}),
    },
  }) + "\\n");
}

process.stdin.resume();
process.stdin.on("data", (chunk) => {
  const text = chunk instanceof Uint8Array ? new TextDecoder().decode(chunk) : String(chunk);
  buffer += text;

  while (true) {
    const newlineIndex = buffer.indexOf("\\n");
    if (newlineIndex === -1) break;
    const line = buffer.slice(0, newlineIndex);
    buffer = buffer.slice(newlineIndex + 1);
    if (!line.trim()) continue;

    const msg = JSON.parse(line);
    if (msg.id === undefined) continue;

    switch (msg.method) {
      case "initialize":
        writeResponse(msg.id, {
          protocolVersion: 1,
          agentInfo: {
            name: "mock-no-update-agent",
            version: "1.0.0",
          },
          agentCapabilities: {
            plan_mode: true,
            tool_calls: false,
            promptCapabilities: {},
          },
          modes: {
            currentModeId: sessionState.modeId,
            availableModes: [
              { id: "default", label: "Default" },
              { id: "plan", label: "Plan" },
            ],
          },
          configOptions: sessionState.configOptions,
        });
        break;
      case "session/new":
        writeResponse(msg.id, {
          sessionId: "mock-session-1",
          modes: {
            currentModeId: sessionState.modeId,
            availableModes: [
              { id: "default", label: "Default" },
              { id: "plan", label: "Plan" },
            ],
          },
          configOptions: sessionState.configOptions,
        });
        break;
      case "session/set_mode":
        sessionState.modeId = msg.params?.modeId ?? sessionState.modeId;
        writeResponse(msg.id, {});
        break;
      case "session/set_config_option": {
        const configId = msg.params?.configId;
        const value = msg.params?.value;
        if (typeof configId !== "string" || typeof value !== "string") {
          writeError(msg.id, "invalid config option params");
          break;
        }
        const option = sessionState.configOptions.find((entry) => entry.id === configId);
        if (!option) {
          writeError(msg.id, "unknown config option", { configId });
          break;
        }
        option.currentValue = value;
        writeResponse(msg.id, { configOptions: sessionState.configOptions });
        break;
      }
      case "session/cancel":
        writeResponse(msg.id, {});
        break;
      default:
        process.stdout.write(JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          error: { code: -32601, message: "Method not found", data: { method: msg.method } },
        }) + "\\n");
        break;
    }
  }
});
`;

describe("synthetic session/update compatibility", () => {
	test("surfaces synthetic mode and config updates when the ACP adapter omits notifications", async () => {
		const agentPackage = createProjectedAgentPackage({
			name: "synthetic",
			adapterScript: MOCK_ACP_ADAPTER,
		});
		const vm = await AgentOs.create({
			defaultSoftware: false,
			software: [agentPackage.software],
		});
		let sessionId: string | undefined;

		try {
			sessionId = "synthetic-updates";
			await vm.openSession({ sessionId, agent: "synthetic" });

			const receivedEvents: string[] = [];
			const unsubscribe = vm.onSessionEvent(sessionId, (event) => {
				if (event.method === "session/update") {
					receivedEvents.push(JSON.stringify(event.params));
				}
			});

			await vm.setSessionModel(sessionId, "gpt-5-codex");
			await vm.setSessionThoughtLevel(sessionId, "high");
			await vm.setSessionMode(sessionId, "plan");
			await new Promise<void>((resolve) => queueMicrotask(resolve));
			unsubscribe();

			expect(vm.getSessionModes(sessionId)?.currentModeId).toBe("plan");
			const configOptions = vm.getSessionConfigOptions(sessionId);
			expect(
				configOptions.find((option) => option.category === "model")
					?.currentValue,
			).toBe("gpt-5-codex");
			expect(
				configOptions.find((option) => option.category === "thought_level")
					?.currentValue,
			).toBe("high");

			expect(
				receivedEvents.some((event) =>
					event.includes('"sessionUpdate":"current_mode_update"'),
				),
			).toBe(true);
			expect(
				receivedEvents.filter((event) =>
					event.includes('"sessionUpdate":"config_option_update"'),
				).length,
			).toBeGreaterThanOrEqual(2);
		} finally {
			if (sessionId) {
				vm.unloadSession({ sessionId });
			}
			await vm.dispose();
			agentPackage.cleanup();
		}
	});
});
