import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
import {
	createProjectedAgentPackage,
	type ProjectedAgentPackage,
} from "./helpers/projected-agent-package.js";

/**
 * Mock ACP adapter that answers initialize/session/new and echoes its launch environment in agentInfo so
 * the test can assert the sidecar-injected system prompt.
 */
const MOCK_ACP_ADAPTER = `
let buffer = '';
process.stdin.resume();
process.stdin.on('data', (chunk) => {
  const str = chunk instanceof Uint8Array ? new TextDecoder().decode(chunk) : String(chunk);
  buffer += str;
  while (true) {
    const idx = buffer.indexOf('\\n');
    if (idx === -1) break;
    const line = buffer.substring(0, idx);
    buffer = buffer.substring(idx + 1);
    if (!line.trim()) continue;
    try {
      const msg = JSON.parse(line);
      if (msg.id === undefined) continue;
      let result;
      switch (msg.method) {
        case 'initialize':
          result = { protocolVersion: 1, agentInfo: { name: 'mock-adapter', version: '1.0', systemPrompt: process.env.ACP_APPEND_SYSTEM_PROMPT || null } };
          break;
        case 'session/new':
          result = { sessionId: 'mock-session-1' };
          break;
        case 'session/cancel':
          result = {};
          break;
        default:
          process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'Method not found' } }) + '\\n');
          continue;
      }
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result }) + '\\n');
    } catch (e) {}
  }
});
`;

const mathFunctions = {
	add: {
		inputSchema: z
			.object({
				a: z.number(),
				b: z.number(),
			})
			.describe("Add two numbers"),
		execute: ({ a, b }) => ({ sum: a + b }),
		examples: [
			{
				description: "Add 1 and 2",
				input: { a: 1, b: 2 },
			},
		],
	},
};

describe("hostFunction reference registration", () => {
	let vm: AgentOs;
	let agentPackage: ProjectedAgentPackage;

	beforeEach(async () => {
		agentPackage = createProjectedAgentPackage({
			name: "pi",
			adapterScript: MOCK_ACP_ADAPTER,
		});
		vm = await AgentOs.create({
			defaultSoftware: false,
			software: [agentPackage.software],
			hostFunctions: { math: mathFunctions },
		});
	});

	afterEach(async () => {
		await vm.dispose();
		agentPackage.cleanup();
	});

	test("stores generated hostFunction reference markdown on the VM", () => {
		const reference = (vm as unknown as { _hostFunctionReference: string })
			._hostFunctionReference;

		expect(reference).toContain("## Available Host Functions");
		expect(reference).toContain(
			"Run `agentos list-host-functions` to see all available host functions.",
		);
		expect(reference).toContain("### math");
		expect(reference).toContain(
			"`agentos-math add --a <number> --b <number>` - Add two numbers",
		);
		expect(reference).toContain("Add 1 and 2");
	});

	test("openSession injects the registered hostFunction reference into the system prompt", async () => {
		const sessionId = "host-function-reference";
		await vm.openSession({ sessionId, agent: "pi" });
		const agentInfo = (await vm.getSessionAgentInfo({ sessionId })) as {
			systemPrompt?: string;
		};
		const prompt = agentInfo.systemPrompt ?? "";
		expect(prompt).toContain("## Available Host Functions");
		expect(prompt).toContain("`agentos-math add --a <number> --b <number>`");
		expect(prompt).toContain("### math");

		await vm.unloadSession({ sessionId });
	});
});
