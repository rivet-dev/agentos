import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import { z } from "zod";
import { AgentOs, hostTool, toolKit } from "../src/index.js";
import type { AgentConfig } from "../src/agents.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");

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

describe("tool reference registration", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create({
			mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
			toolKits: [mathToolKit],
		});
	});

	afterEach(async () => {
		await vm.dispose();
	});

	function useMockAdapterBin(scriptPath: string): () => void {
		const priv = vm as unknown as {
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

	test("stores generated tool reference markdown on the VM", () => {
		const toolReference = (vm as unknown as { _toolReference: string })
			._toolReference;

		expect(toolReference).toContain("## Available Host Tools");
		expect(toolReference).toContain(
			"Run `agentos list-tools` to see all available tools.",
		);
		expect(toolReference).toContain("### math");
		expect(toolReference).toContain("Math utilities");
		expect(toolReference).toContain(
			"`agentos-math add --a <number> --b <number>`",
		);
		expect(toolReference).toContain("Add 1 and 2");
	});

	test("createSession injects the registered tool reference into the system prompt", async () => {
		const scriptPath = "/tmp/mock-tool-reference-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const { sessionId } = await vm.createSession("pi");
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			const argIndex = argv.indexOf("--append-system-prompt");
			expect(argIndex).toBeGreaterThan(-1);
			const prompt = argv[argIndex + 1];
			expect(prompt).toContain("## Available Host Tools");
			expect(prompt).toContain("`agentos-math add --a <number> --b <number>`");
			expect(prompt).toContain("### math");

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});
});
