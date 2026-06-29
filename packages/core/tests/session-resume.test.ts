import { resolve } from "node:path";
import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type { AgentConfig } from "../src/agents.js";
import type { SoftwareInput } from "../src/packages.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";

// L2 (agent-os side): exercise the sidecar resume orchestration state machine
// end-to-end against the REAL agentos-sidecar with a MOCK ACP adapter (no LLM).
//
// Spec: .agent/specs/session-resume.md §6
//   - Tier 1 (native): agent advertises `loadSession` -> sidecar runs
//     `initialize` then `session/load`; on success mode "native", id preserved.
//   - unknown_session fallthrough: `session/load` returns the OpenCode-shape error
//     ({code:-32603, data:{details:"NotFoundError"}}) or data.kind=="unknown_session"
//     -> fall through to Tier 2.
//   - Tier 2 (fallback): `session/new`, mode "fallback", new live id, and a
//     continuation preamble prepended to the next `session/prompt`.

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const MOCK_ADAPTER_PATH = "/tmp/mock-session-resume-adapter.mjs";

const SYNTHETIC_AGENT = {
	name: "session-resume-mock",
	type: "agent" as const,
	packageDir: MODULE_ACCESS_CWD,
	requires: [],
	agent: {
		id: "synthetic",
		acpAdapter: "session-resume-mock-adapter",
		agentPackage: "session-resume-mock-agent",
	},
};

// Single configurable mock ACP adapter. Its behavior is selected at launch time
// via the `MOCK_RESUME_SCENARIO` env var, which the host forwards through
// `resumeSession(..., { env })` (the sidecar passes `env` straight to the
// adapter process). Scenarios:
//   - "native":      advertise loadSession; session/load -> ok
//   - "resume-only": advertise resume; session/resume -> ok
//   - "fallthrough": advertise loadSession; session/load -> NotFoundError error
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
          agentCapabilities.resume = true;
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
          // OpenCode-shape "no such session" sentinel: -32603 + NotFoundError.
          writeError(msg.id, -32603, "Internal error", {
            details: "NotFoundError",
          });
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

function useMockAdapterBin(vm: AgentOs, scriptPath: string): () => void {
	const priv = vm as AgentOs & {
		_resolveAgentConfig: (id: string) => AgentConfig | undefined;
	};
	const originalConfig = priv._resolveAgentConfig.bind(priv);
	priv._resolveAgentConfig = (id: string) => {
		const c = originalConfig(id);
		return c
			? { ...c, adapterEntrypoint: scriptPath }
			: { adapterEntrypoint: scriptPath };
	};
	return () => {
		priv._resolveAgentConfig = originalConfig;
	};
}

async function createMockAgentVm(software: SoftwareInput[]): Promise<AgentOs> {
	return AgentOs.create({
		mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
		software,
	});
}

describe("sidecar resume orchestration (mock ACP adapter)", () => {
	test("Tier 1 native: loadSession advertised + session/load ok -> mode native, id preserved", async () => {
		const vm = await createMockAgentVm([SYNTHETIC_AGENT]);
		const restore = useMockAdapterBin(vm, MOCK_ADAPTER_PATH);
		let liveSessionId: string | undefined;

		try {
			await vm.writeFile(MOCK_ADAPTER_PATH, MOCK_ACP_ADAPTER);

			const externalSessionId = "external-session-native";
			const result = await vm.resumeSession(externalSessionId, "synthetic", {
				env: { MOCK_RESUME_SCENARIO: "native" },
			});
			liveSessionId = result.sessionId;

			expect(result.mode).toBe("native");
			// Native load reuses the requested id: external == live.
			expect(result.sessionId).toBe(externalSessionId);
		} finally {
			restore();
			if (liveSessionId) {
				vm.closeSession(liveSessionId);
			}
			await vm.dispose();
		}
	});

	test("Tier 1 native: resume advertised + session/resume ok -> mode native, id preserved", async () => {
		const vm = await createMockAgentVm([SYNTHETIC_AGENT]);
		const restore = useMockAdapterBin(vm, MOCK_ADAPTER_PATH);
		let liveSessionId: string | undefined;

		try {
			await vm.writeFile(MOCK_ADAPTER_PATH, MOCK_ACP_ADAPTER);

			const externalSessionId = "external-session-resume-only";
			const result = await vm.resumeSession(externalSessionId, "synthetic", {
				env: { MOCK_RESUME_SCENARIO: "resume-only" },
			});
			liveSessionId = result.sessionId;

			expect(result.mode).toBe("native");
			expect(result.sessionId).toBe(externalSessionId);
		} finally {
			restore();
			if (liveSessionId) {
				vm.closeSession(liveSessionId);
			}
			await vm.dispose();
		}
	});

	test("unknown_session fallthrough: session/load NotFoundError -> mode fallback, new live id, preamble prepended", async () => {
		const vm = await createMockAgentVm([SYNTHETIC_AGENT]);
		const restore = useMockAdapterBin(vm, MOCK_ADAPTER_PATH);
		let liveSessionId: string | undefined;

		try {
			await vm.writeFile(MOCK_ADAPTER_PATH, MOCK_ACP_ADAPTER);

			const externalSessionId = "external-session-fallthrough";
			const transcriptPath = "/root/.agentos/threads/external-session-fallthrough.md";
			const result = await vm.resumeSession(externalSessionId, "synthetic", {
				transcriptPath,
				env: { MOCK_RESUME_SCENARIO: "fallthrough" },
			});
			liveSessionId = result.sessionId;

			// session/load returned the unknown_session sentinel, so the sidecar fell
			// through to Tier 2: a fresh session/new id, mode "fallback".
			expect(result.mode).toBe("fallback");
			expect(result.sessionId).toBe("mock-fallback-session");
			expect(result.sessionId).not.toBe(externalSessionId);

			// The next session/prompt must arrive at the adapter with the continuation
			// preamble prepended as a leading text block. The adapter echoes the exact
			// prompt blocks it received back as the agent message text.
			const { text } = await vm.prompt(liveSessionId, "what did we discuss?");
			const receivedBlocks = JSON.parse(text) as Array<{
				type: string;
				text: string;
			}>;

			expect(receivedBlocks.length).toBe(2);
			// Leading block is the injected preamble pointing at the transcript path.
			expect(receivedBlocks[0].type).toBe("text");
			expect(receivedBlocks[0].text).toContain(
				"You are continuing an earlier session",
			);
			expect(receivedBlocks[0].text).toContain(transcriptPath);
			// Original user text follows.
			expect(receivedBlocks[1].text).toBe("what did we discuss?");

			// Preamble is single-turn: a second prompt has no leading preamble block.
			const second = await vm.prompt(liveSessionId, "second turn");
			const secondBlocks = JSON.parse(second.text) as Array<{
				type: string;
				text: string;
			}>;
			expect(secondBlocks.length).toBe(1);
			expect(secondBlocks[0].text).toBe("second turn");
		} finally {
			restore();
			if (liveSessionId) {
				vm.closeSession(liveSessionId);
			}
			await vm.dispose();
		}
	});

	test("no loadSession capability -> straight to fallback (session/new)", async () => {
		const vm = await createMockAgentVm([SYNTHETIC_AGENT]);
		const restore = useMockAdapterBin(vm, MOCK_ADAPTER_PATH);
		let liveSessionId: string | undefined;

		try {
			await vm.writeFile(MOCK_ADAPTER_PATH, MOCK_ACP_ADAPTER);

			const externalSessionId = "external-session-nocap";
			const result = await vm.resumeSession(externalSessionId, "synthetic", {
				env: { MOCK_RESUME_SCENARIO: "no-loadsession" },
			});
			liveSessionId = result.sessionId;

			// No loadSession capability -> Tier 1 is skipped entirely; fallback runs.
			expect(result.mode).toBe("fallback");
			expect(result.sessionId).toBe("mock-fallback-session");

			// No transcriptPath was supplied, so no preamble is armed.
			const { text } = await vm.prompt(liveSessionId, "hello");
			const blocks = JSON.parse(text) as Array<{ type: string; text: string }>;
			expect(blocks.length).toBe(1);
			expect(blocks[0].text).toBe("hello");
		} finally {
			restore();
			if (liveSessionId) {
				vm.closeSession(liveSessionId);
			}
			await vm.dispose();
		}
	});
});
