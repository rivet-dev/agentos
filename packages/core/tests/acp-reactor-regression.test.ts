import { resolve } from "node:path";
import common from "@agentos-software/common";
import { afterEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, hostTool, toolKit } from "../src/index.js";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import { createProjectedAgentPackage } from "./helpers/projected-agent-package.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const UPDATE_COUNT = 256;
const textDecoder = new TextDecoder();

// This adapter recreates the original failure across the complete production
// path. The child command starts before the notification burst, so its delayed
// fd3 host-tool response becomes ready while the ordinary ACP event lane is
// carrying 256 session/update notifications.
const ACP_REACTOR_ADAPTER = String.raw`
const { spawn } = require("node:child_process");

let input = "";
let promptCount = 0;
let dispatchTail = Promise.resolve();

function writeMessage(message) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

function writeResponse(id, result) {
  writeMessage({ jsonrpc: "2.0", id, result });
}

function writeUpdate(text) {
  writeMessage({
    jsonrpc: "2.0",
    method: "session/update",
    params: {
      sessionId: "reactor-session-1",
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { text },
      },
    },
  });
}

function runMathTool() {
  const child = spawn(
    "agentos-math",
    ["add", "--a", "19", "--b", "23"],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += String(chunk);
  });
  child.stderr.on("data", (chunk) => {
    stderr += String(chunk);
  });
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code) => {
      if (code !== 0) {
        reject(new Error("agentos-math exited " + String(code) + ": " + stderr));
        return;
      }
      try {
        resolve(JSON.parse(stdout));
      } catch (error) {
        reject(new Error("invalid agentos-math output: " + stdout + ": " + String(error)));
      }
    });
  });
}

async function handleMessage(msg) {
  if (msg.id === undefined) return;

  switch (msg.method) {
    case "initialize":
      writeResponse(msg.id, {
        protocolVersion: 1,
        agentInfo: { name: "acp-reactor-regression", version: "1.0.0" },
        agentCapabilities: {
          plan_mode: false,
          tool_calls: false,
          promptCapabilities: {},
        },
        modes: {
          currentModeId: "default",
          availableModes: [{ id: "default", label: "Default" }],
        },
        configOptions: [],
      });
      return;
    case "session/new":
      writeResponse(msg.id, {
        sessionId: "reactor-session-1",
        modes: {
          currentModeId: "default",
          availableModes: [{ id: "default", label: "Default" }],
        },
        configOptions: [],
      });
      return;
    case "session/prompt": {
      promptCount += 1;
      if (promptCount === 1) {
        // Deliberately do not await here: the host callback must be in flight
        // while the ordinary notification lane receives the complete burst.
        const toolResultPromise = runMathTool();
        for (let index = 0; index < ${UPDATE_COUNT}; index += 1) {
          writeUpdate("reactor-update:" + String(index));
        }
        const toolEnvelope = await toolResultPromise;
        if (
          toolEnvelope === null ||
          toolEnvelope.ok !== true ||
          toolEnvelope.result === null ||
          toolEnvelope.result.sum !== 42
        ) {
          throw new Error("unexpected host tool result: " + JSON.stringify(toolEnvelope));
        }
        writeUpdate("reactor-tool-result:" + String(toolEnvelope.result.sum));
        writeResponse(msg.id, { stopReason: "end_turn" });
        return;
      }

      writeUpdate("reactor-session-reused:" + String(promptCount));
      writeResponse(msg.id, { stopReason: "end_turn" });
      return;
    }
    case "session/cancel":
      writeResponse(msg.id, {});
      return;
    default:
      writeMessage({
        jsonrpc: "2.0",
        id: msg.id,
        error: {
          code: -32601,
          message: "Method not found",
          data: { method: msg.method },
        },
      });
  }
}

process.stdin.resume();
process.stdin.on("data", (chunk) => {
  input += String(chunk);
  while (true) {
    const newline = input.indexOf("\n");
    if (newline === -1) break;
    const line = input.slice(0, newline);
    input = input.slice(newline + 1);
    if (!line.trim()) continue;
    const message = JSON.parse(line);
    dispatchTail = dispatchTail.then(() => handleMessage(message)).catch((error) => {
      process.stderr.write(String(error && error.stack ? error.stack : error) + "\n");
      process.exitCode = 1;
    });
  }
});
`.trim();

async function waitFor(
	predicate: () => boolean,
	timeoutMs = 10_000,
): Promise<void> {
	const deadline = performance.now() + timeoutMs;
	while (!predicate()) {
		if (performance.now() >= deadline) {
			throw new Error(`condition was not met within ${timeoutMs}ms`);
		}
		await new Promise<void>((resolveWait) => setTimeout(resolveWait, 10));
	}
}

describe("ACP adapter reactor regression", () => {
	const cleanups = new Set<() => Promise<void>>();

	afterEach(async () => {
		for (const cleanup of cleanups) {
			await cleanup();
		}
		cleanups.clear();
	});

	test("routes a delayed host-tool response past 256 ordinary updates and keeps the session reusable", async () => {
		let hostToolCalls = 0;
		const hostToolInputs: Array<{ a: number; b: number }> = [];
		const mathToolKit = toolKit({
			name: "math",
			description: "Math utilities",
			tools: {
				add: hostTool({
					description: "Add two numbers",
					inputSchema: z.object({
						a: z.number(),
						b: z.number(),
					}),
					execute: async ({ a, b }) => {
						hostToolCalls += 1;
						hostToolInputs.push({ a, b });
						await new Promise<void>((resolveDelay) =>
							setTimeout(resolveDelay, 50),
						);
						return { sum: a + b };
					},
				}),
			},
		});
		const agentPackage = createProjectedAgentPackage({
			name: "acp-reactor-regression",
			adapterScript: ACP_REACTOR_ADAPTER,
		});
		cleanups.add(async () => agentPackage.cleanup());

		const stderrChunks: string[] = [];
		const unexpectedAgentExits: unknown[] = [];
		const vm = await AgentOs.create({
			mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
			defaultSoftware: false,
			software: [common, agentPackage.software],
			toolKits: [mathToolKit],
			permissions: {
				fs: "allow",
				childProcess: "allow",
				binding: "allow",
			},
			onAgentStderr: (event) => {
				stderrChunks.push(textDecoder.decode(event.chunk));
			},
			onAgentExit: (event) => {
				unexpectedAgentExits.push(event);
			},
		});
		cleanups.add(async () => vm.dispose());

		const { sessionId } = await vm.createSession("acp-reactor-regression");
		const updateTexts: string[] = [];
		const unsubscribe = vm.onSessionEvent(sessionId, (event) => {
			if (event.method !== "session/update") return;
			const params = event.params as {
				update?: { content?: { text?: unknown } };
			};
			const text = params.update?.content?.text;
			if (typeof text === "string") updateTexts.push(text);
		});

		try {
			const firstPrompt = await vm.prompt(
				sessionId,
				"Exercise the saturated ordinary event lane.",
			);
			expect(firstPrompt.response.error).toBeUndefined();
			expect(
				(firstPrompt.response.result as { stopReason?: string }).stopReason,
			).toBe("end_turn");
			await waitFor(
				() =>
					updateTexts.filter((text) => text.startsWith("reactor-update:"))
						.length === UPDATE_COUNT &&
					updateTexts.includes("reactor-tool-result:42"),
			);

			const numberedUpdates = updateTexts
				.filter((text) => text.startsWith("reactor-update:"))
				.map((text) => Number(text.slice("reactor-update:".length)));
			expect(numberedUpdates).toEqual(
				Array.from({ length: UPDATE_COUNT }, (_, index) => index),
			);
			expect(new Set(numberedUpdates).size).toBe(UPDATE_COUNT);
			expect(hostToolCalls).toBe(1);
			expect(hostToolInputs).toEqual([{ a: 19, b: 23 }]);

			const secondPrompt = await vm.prompt(
				sessionId,
				"Prove that the same adapter session remains reusable.",
			);
			expect(secondPrompt.response.error).toBeUndefined();
			expect(
				(secondPrompt.response.result as { stopReason?: string }).stopReason,
			).toBe("end_turn");
			await waitFor(() => updateTexts.includes("reactor-session-reused:2"));
			expect(hostToolCalls).toBe(1);
			expect(unexpectedAgentExits).toEqual([]);

			const stderr = stderrChunks.join("");
			expect(stderr).not.toMatch(/sync bridge deferred message queue/i);
			expect(stderr).not.toMatch(/bridge(?: response)? rout/i);
			expect(stderr).not.toMatch(/adapter process .*exited with code/i);
			expect(stderr).not.toMatch(/session evicted/i);
		} finally {
			unsubscribe();
			vm.closeSession(sessionId);
		}
	}, 120_000);
});
