import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { createProjectedAgentPackage } from "./helpers/projected-agent-package.js";

// Exercise lazy durable restoration through the public API against the real
// sidecar and a mock ACP adapter. `unloadSession` hides the private ACP id;
// `prompt` must restore it natively or create a continuation session.

// Single configurable mock ACP adapter. Its behavior is selected at launch time
// via the immutable session env. Scenarios:
//   - "native":      advertise loadSession; session/load -> ok
//   - "resume-only": advertise resume; session/resume -> ok
//   - "fallthrough": advertise loadSession; session/load -> ACP ResourceNotFound
//   - "no-loadsession": do NOT advertise loadSession (straight to fallback)
//
// On `session/prompt` the adapter echoes the exact prompt blocks it received back
// as an `agent_message_chunk` text update so the test can assert on the
// continuation preamble being prepended.
const MOCK_ACP_ADAPTER = String.raw`
let buffer = "";

const SCENARIO = process.env.MOCK_RESUME_SCENARIO || "native";
const NATIVE_SESSION_ID = "mock-native-session";
const FALLBACK_SESSION_ID = "mock-fallback-session";

const modes = {
  currentModeId: "default",
  availableModes: [
    { id: "default", label: "Default" },
    { id: "plan", label: "Plan" },
  ],
};

function write(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}

function writeResponse(id, result) {
  write({ jsonrpc: "2.0", id, result });
}

function writeError(id, code, message, data) {
  write({
    jsonrpc: "2.0",
    id,
    error: { code, message, ...(data ? { data } : {}) },
  });
}

function writeNotification(method, params) {
  write({ jsonrpc: "2.0", method, params });
}

process.stdin.resume();
process.stdin.on("data", (chunk) => {
  const text =
    chunk instanceof Uint8Array ? new TextDecoder().decode(chunk) : String(chunk);
  buffer += text;

  while (true) {
    const newlineIndex = buffer.indexOf("\n");
    if (newlineIndex === -1) break;
    const line = buffer.slice(0, newlineIndex);
    buffer = buffer.slice(newlineIndex + 1);
    if (!line.trim()) continue;

    const msg = JSON.parse(line);
    if (msg.id === undefined) continue;

    switch (msg.method) {
      case "initialize": {
        const agentCapabilities = {
          promptCapabilities: {},
        };
        if (SCENARIO === "resume-only") {
          agentCapabilities.sessionCapabilities = { resume: {} };
        } else if (SCENARIO !== "no-loadsession") {
          agentCapabilities.loadSession = true;
        }
        writeResponse(msg.id, {
          protocolVersion: 1,
          agentInfo: { name: "mock-resume-agent", version: "1.0.0" },
          agentCapabilities,
          modes,
        });
        break;
      }
      case "session/load": {
        if (SCENARIO === "native") {
          // Native resume reuses the requested session id (the sidecar keeps
          // request.session_id for native loads); just acknowledge success.
          writeResponse(msg.id, { modes });
        } else if (SCENARIO === "fallthrough") {
          writeError(msg.id, -32002, "Resource not found");
        } else {
          writeError(msg.id, -32601, "Method not found", {
            method: "session/load",
          });
        }
        break;
      }
      case "session/resume": {
        if (SCENARIO === "resume-only") {
          writeResponse(msg.id, { modes });
        } else {
          writeError(msg.id, -32601, "Method not found", {
            method: "session/resume",
          });
        }
        break;
      }
      case "session/new": {
        writeResponse(msg.id, { sessionId: FALLBACK_SESSION_ID, modes });
        break;
      }
      case "session/prompt": {
        // Echo the exact prompt blocks the adapter received as a message chunk so
        // the host (and test) can observe any sidecar-prepended preamble.
        const blocks = Array.isArray(msg.params?.prompt) ? msg.params.prompt : [];
        writeNotification("session/update", {
          sessionId: msg.params?.sessionId,
          update: {
            sessionUpdate: "agent_message_chunk",
            content: { type: "text", text: JSON.stringify(blocks) },
          },
        });
        writeResponse(msg.id, { stopReason: "end_turn" });
        break;
      }
      case "session/cancel":
        writeResponse(msg.id, {});
        break;
      default:
        writeError(msg.id, -32601, "Method not found", { method: msg.method });
        break;
    }
  }
});
`;

async function createMockAgentVm(): Promise<{
	vm: AgentOs;
	cleanup(): void;
}> {
	const agentPackage = createProjectedAgentPackage({
		name: "synthetic",
		adapterScript: MOCK_ACP_ADAPTER,
	});
	const vm = await AgentOs.create({
		defaultSoftware: false,
		software: [agentPackage.software],
	});
	return {
		vm,
		cleanup: agentPackage.cleanup,
	};
}

describe("sidecar resume orchestration (mock ACP adapter)", () => {
	let vm: AgentOs;
	let cleanup: () => void;

	beforeAll(async () => {
		({ vm, cleanup } = await createMockAgentVm());
	}, 120_000);

	afterAll(async () => {
		await vm.dispose();
		cleanup();
	}, 120_000);

	async function textPrompt(sessionId: string, text: string) {
		return vm.prompt({
			sessionId,
			content: [{ type: "text", text }],
		});
	}

	test.each([
		["loadSession", "native"],
		["session/resume", "resume-only"],
	] as const)("restores through native %s while preserving the public id", async (_method, scenario) => {
		const sessionId = `public-${scenario}`;
		try {
			expect(
				await vm.openSession({
					sessionId,
					agent: "synthetic",
					env: { MOCK_RESUME_SCENARIO: scenario },
				}),
			).toBeUndefined();
			await vm.unloadSession({ sessionId });
			const restored = await textPrompt(sessionId, "after unload");
			expect(restored.sessionId).toBe(sessionId);
			expect(JSON.parse(restored.text)).toEqual([
				{ type: "text", text: "after unload" },
			]);
		} finally {
			await vm.deleteSession({ sessionId });
		}
	});

	test.each([
		"fallthrough",
		"no-loadsession",
	] as const)("uses bounded SQLite continuation for %s fallback", async (scenario) => {
		const sessionId = `public-${scenario}`;
		try {
			await vm.openSession({
				sessionId,
				agent: "synthetic",
				env: { MOCK_RESUME_SCENARIO: scenario },
			});
			await textPrompt(sessionId, "first turn");
			await vm.unloadSession({ sessionId });
			const restored = await textPrompt(sessionId, "second turn");
			const blocks = JSON.parse(restored.text) as Array<{
				type: string;
				text: string;
			}>;
			expect(blocks).toHaveLength(2);
			expect(blocks[0]?.text).toContain(
				"authoritative recent ACP session updates",
			);
			expect(blocks[0]?.text).toContain("first turn");
			expect(blocks[1]?.text).toBe("second turn");
		} finally {
			await vm.deleteSession({ sessionId });
		}
	});
});
