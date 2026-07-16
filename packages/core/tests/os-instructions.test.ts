import * as fs from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import {
	createProjectedAgentPackage,
	type ProjectedAgentPackage,
} from "./helpers/projected-agent-package.js";

const OS_INSTRUCTIONS_FIXTURE = resolve(
	import.meta.dirname,
	// The sidecar crate embeds this prompt; it lives next to the Rust source so
	// `cargo publish` can package it. This test only sanity-checks its contents.
	"../../../crates/agentos-sidecar/src/AGENTOS_SYSTEM_PROMPT.md",
);

// ── base prompt fixture sanity ─────────────────────────────────────────
//
// The base prompt is no longer baked into a guest file. The sidecar embeds this fixture and
// injects it at session start. This block only verifies the fixture itself is non-empty so the
// injection has real content to assemble.

describe("base system prompt fixture", () => {
	test("ships a non-empty base prompt", () => {
		const base = fs.readFileSync(OS_INSTRUCTIONS_FIXTURE, "utf-8");
		expect(base).toBeTruthy();
		expect(base.length).toBeGreaterThan(0);
		expect(base).toContain("# agentOS");
	});
});

// ── createSession integration tests ────────────────────────────────────

/**
 * Mock ACP adapter that responds to initialize/session/new.
 * Echoes process.env in agentInfo for env var verification.
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
          result = {
            protocolVersion: 1,
            agentInfo: {
              name: 'mock-adapter',
              version: '1.0',
              contextPaths: process.env.OPENCODE_CONTEXTPATHS || null,
              systemPrompt: process.env.ACP_APPEND_SYSTEM_PROMPT || null,
              argv: process.argv.slice(2),
            },
          };
          break;
        case 'session/new':
          result = { sessionId: 'mock-session-1' };
          break;
        case 'session/cancel':
          result = {};
          break;
        default:
          process.stdout.write(JSON.stringify({
            jsonrpc: '2.0', id: msg.id,
            error: { code: -32601, message: 'Method not found' },
          }) + '\\n');
          continue;
      }

      process.stdout.write(JSON.stringify({
        jsonrpc: '2.0', id: msg.id, result,
      }) + '\\n');
    } catch (e) {}
  }
});
`;

describe("createSession OS instructions integration", () => {
	let vm: AgentOs;
	let agentPackages: ProjectedAgentPackage[];

	beforeEach(async () => {
		agentPackages = ["pi", "opencode"].map((name) =>
			createProjectedAgentPackage({ name, adapterScript: MOCK_ACP_ADAPTER }),
		);
		vm = await AgentOs.create({
			defaultSoftware: false,
			software: agentPackages.map((agentPackage) => agentPackage.software),
		});
	});

	afterEach(async () => {
		await vm.dispose();
		for (const agentPackage of agentPackages) {
			agentPackage.cleanup();
		}
	});

	test("createSession passes the assembled prompt through the adapter-neutral contract", async () => {
		const { sessionId } = await vm.createSession("pi");
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			systemPrompt?: string;
		};
		const instructionsArg = agentInfo.systemPrompt ?? "";
		expect(instructionsArg).toBeTruthy();
		expect(instructionsArg.length).toBeGreaterThan(0);
		// The sidecar injects the embedded base prompt, not a guest-read file.
		expect(instructionsArg).toContain("# agentOS");

		vm.closeSession(sessionId);
	});

	test("createSession leaves OpenCode compatibility translation to its launcher", async () => {
		const { sessionId } = await vm.createSession("opencode");

		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			contextPaths?: string;
			systemPrompt?: string;
			argv?: string[];
		};
		expect(agentInfo.argv ?? []).not.toContain("acp");
		expect(agentInfo.systemPrompt).toContain("# agentOS");
		expect(agentInfo.contextPaths).toBeNull();

		// No .agent-os/ directory created in cwd
		const agentOsDirExists = await vm.exists("/home/agentos/.agent-os");
		expect(agentOsDirExists).toBe(false);

		vm.closeSession(sessionId);
	});

	test("createSession with skipOsInstructions:true does not inject args or env", async () => {
		const { sessionId } = await vm.createSession("pi", {
			skipOsInstructions: true,
		});
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			systemPrompt?: string;
		};
		expect(agentInfo.systemPrompt).toBeNull();

		vm.closeSession(sessionId);
	});

	test("createSession with skipOsInstructions:true still forwards additionalInstructions", async () => {
		const additionalText = "CUSTOM_MARKER: skip base, keep extras.";

		const { sessionId } = await vm.createSession("pi", {
			skipOsInstructions: true,
			additionalInstructions: additionalText,
		});
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			systemPrompt?: string;
		};
		const instructionsArg = agentInfo.systemPrompt ?? "";
		expect(instructionsArg).toContain(additionalText);
		expect(instructionsArg).not.toContain("# agentOS");

		vm.closeSession(sessionId);
	});

	test("user-provided env vars override instruction env vars", async () => {
		const userContextPaths = '["my-custom-paths.md"]';
		const { sessionId } = await vm.createSession("opencode", {
			env: { OPENCODE_CONTEXTPATHS: userContextPaths },
		});

		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			contextPaths?: string;
		};
		expect(agentInfo.contextPaths).toBe(userContextPaths);

		vm.closeSession(sessionId);
	});

	test("additionalInstructions content appears in injected text", async () => {
		const additionalText = "CUSTOM_MARKER: Always use pnpm for this project.";

		const { sessionId } = await vm.createSession("pi", {
			additionalInstructions: additionalText,
		});
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			systemPrompt?: string;
		};
		const instructionsArg = agentInfo.systemPrompt ?? "";
		expect(instructionsArg).toContain(additionalText);

		vm.closeSession(sessionId);
	});

	test("AgentOs.create additionalInstructions are included in created sessions", async () => {
		await vm.dispose();
		const vmLevelInstructions =
			"CUSTOM_MARKER: VM-level instruction applies to every session.";
		vm = await AgentOs.create({
			defaultSoftware: false,
			additionalInstructions: vmLevelInstructions,
			software: agentPackages.map((agentPackage) => agentPackage.software),
		});

		const { sessionId } = await vm.createSession("pi");
		const agentInfo = vm.getSessionAgentInfo(sessionId) as {
			systemPrompt?: string;
		};
		const instructionsArg = agentInfo.systemPrompt ?? "";
		expect(instructionsArg).toContain(vmLevelInstructions);

		vm.closeSession(sessionId);
	});
});
