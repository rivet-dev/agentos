import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { z } from "zod";
import { AgentOs, binding, bindings } from "../src/index.js";
import {
	createProjectedAgentPackage,
	type ProjectedAgentPackage,
} from "./helpers/projected-agent-package.js";

/**
 * Mock ACP adapter that answers initialize/session/new and echoes its launch argv in agentInfo so
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
          result = { protocolVersion: 1, agentInfo: { name: 'mock-adapter', version: '1.0', argv: process.argv.slice(2) } };
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

const mathBindings = bindings({
	name: "math",
	description: "Math utilities",
	bindings: {
		add: binding({
			description: "Add two numbers",
			inputSchema: z.object({
				a: z.number(),
				b: z.number(),
			}),
			execute: ({ a, b }) => ({ sum: a + b }),
			examples: [
				{
					description: "Add 1 and 2",
					input: { a: 1, b: 2 },
				},
			],
		}),
	},
});

describe("binding reference registration", () => {
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
			bindings: [mathBindings],
		});
	});

	afterEach(async () => {
		await vm.dispose();
		agentPackage.cleanup();
	});

	test("stores generated binding reference markdown on the VM", () => {
		const bindingReference = (vm as unknown as { _bindingReference: string })
			._bindingReference;

		expect(bindingReference).toContain("## Available Host Bindings");
		expect(bindingReference).toContain(
			"Run `agentos list-bindings` to see all available bindings.",
		);
		expect(bindingReference).toContain("### math");
		expect(bindingReference).toContain("Math utilities");
		expect(bindingReference).toContain(
			"`agentos-math add --a <number> --b <number>`",
		);
		expect(bindingReference).toContain("Add 1 and 2");
	});

	test("createSession injects the registered binding reference into the system prompt", async () => {
		const { sessionId } = await vm.createSession("pi");
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			argv?: string[];
		};
		const argv = agentInfo.argv ?? [];

		const argIndex = argv.indexOf("--append-system-prompt");
		expect(argIndex).toBeGreaterThan(-1);
		const prompt = argv[argIndex + 1];
		expect(prompt).toContain("## Available Host Bindings");
		expect(prompt).toContain("`agentos-math add --a <number> --b <number>`");
		expect(prompt).toContain("### math");

		vm.closeSession(sessionId);
	});
});
