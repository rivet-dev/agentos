import * as fs from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

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

	beforeEach(async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
		});
	});

	afterEach(async () => {
		await vm.dispose();
	});

	/**
	 * Patch _resolveAdapterBin to return a mock script path instead of
	 * resolving the real adapter from node_modules.
	 */
	function useMockAdapterBin(scriptPath: string): () => void {
		const privateVm = vm as unknown as {
			_resolveAdapterBin: (pkg: string) => string;
			_resolvePackageBin: (pkg: string, bin?: string) => string;
		};
		const origResolveAdapter = (
			vm as unknown as { _resolveAdapterBin: (pkg: string) => string }
		)._resolveAdapterBin;
		const origResolvePackageBin = privateVm._resolvePackageBin;
		privateVm._resolveAdapterBin = (_pkg: string) => scriptPath;
		privateVm._resolvePackageBin = (_pkg: string, _bin?: string) => "/tmp/mock-bin";

		return () => {
			privateVm._resolveAdapterBin = origResolveAdapter;
			privateVm._resolvePackageBin = origResolvePackageBin;
		};
	}

	test("createSession with PI passes --append-system-prompt in spawn args", async () => {
		const scriptPath = "/tmp/mock-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const { sessionId } = await vm.createSession("pi");
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			expect(argv).toContain("--append-system-prompt");
			const argIdx = argv.indexOf("--append-system-prompt");
			const instructionsArg = argv[argIdx + 1];
			expect(instructionsArg).toBeTruthy();
			expect(instructionsArg.length).toBeGreaterThan(0);
			// The sidecar injects the embedded base prompt, not a guest-read file.
			expect(instructionsArg).toContain("# agentOS");

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("createSession with OpenCode passes the sidecar-materialized prompt path in OPENCODE_CONTEXTPATHS", async () => {
		const scriptPath = "/tmp/mock-opencode-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const { sessionId } = await vm.createSession("opencode");

			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				contextPaths?: string;
				argv?: string[];
			};
			const contextPaths = JSON.parse(agentInfo.contextPaths as string);
			expect(agentInfo.argv ?? []).not.toContain("acp");
			// The base prompt is injected through a sidecar-materialized file, not the old baked path.
			expect(contextPaths).toContain("/tmp/agentos-system-prompt.md");
			expect(contextPaths).not.toContain("/etc/agentos/instructions.md");
			// Default opencode repo-relative markers are still present.
			expect(contextPaths).toContain("CLAUDE.md");
			expect(contextPaths).toContain("opencode.md");

			// The materialized prompt file holds the base prompt text.
			const promptData = await vm.readFile("/tmp/agentos-system-prompt.md");
			const promptText = new TextDecoder().decode(promptData);
			expect(promptText).toContain("# agentOS");

			// No .agent-os/ directory created in cwd
			const agentOsDirExists = await vm.exists("/home/agentos/.agent-os");
			expect(agentOsDirExists).toBe(false);

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("createSession with skipOsInstructions:true does not inject args or env", async () => {
		const scriptPath = "/tmp/mock-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const { sessionId } = await vm.createSession("pi", {
				skipOsInstructions: true,
			});
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			expect(argv).not.toContain("--append-system-prompt");

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("createSession with skipOsInstructions:true still forwards additionalInstructions", async () => {
		const scriptPath = "/tmp/mock-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		const additionalText = "CUSTOM_MARKER: skip base, keep extras.";

		try {
			const { sessionId } = await vm.createSession("pi", {
				skipOsInstructions: true,
				additionalInstructions: additionalText,
			});
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			const argIdx = argv.indexOf("--append-system-prompt");
			expect(argIdx).toBeGreaterThan(-1);
			const instructionsArg = argv[argIdx + 1];
			expect(instructionsArg).toContain(additionalText);
			expect(instructionsArg).not.toContain("# agentOS");

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("user-provided env vars override instruction env vars", async () => {
		const scriptPath = "/tmp/mock-opencode-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const userContextPaths = '["my-custom-paths.md"]';
			const { sessionId } = await vm.createSession("opencode", {
				env: { OPENCODE_CONTEXTPATHS: userContextPaths },
			});

			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				contextPaths?: string;
			};
			expect(agentInfo.contextPaths).toBe(userContextPaths);

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("additionalInstructions content appears in injected text", async () => {
		const scriptPath = "/tmp/mock-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		const additionalText = "CUSTOM_MARKER: Always use pnpm for this project.";

		try {
			const { sessionId } = await vm.createSession("pi", {
				additionalInstructions: additionalText,
			});
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			const argIdx = argv.indexOf("--append-system-prompt");
			expect(argIdx).toBeGreaterThan(-1);
			const instructionsArg = argv[argIdx + 1];
			expect(instructionsArg).toContain(additionalText);

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});

	test("AgentOs.create additionalInstructions are included in created sessions", async () => {
		await vm.dispose();
		const vmLevelInstructions =
			"CUSTOM_MARKER: VM-level instruction applies to every session.";
		vm = await AgentOs.create({
			defaultSoftware: false,
			additionalInstructions: vmLevelInstructions,
		});

		const scriptPath = "/tmp/mock-adapter.mjs";
		await vm.writeFile(scriptPath, MOCK_ACP_ADAPTER);
		const restore = useMockAdapterBin(scriptPath);

		try {
			const { sessionId } = await vm.createSession("pi");
			const agentInfo = vm.getSessionAgentInfo(sessionId) as {
				argv?: string[];
			};
			const argv = agentInfo.argv ?? [];

			const argIdx = argv.indexOf("--append-system-prompt");
			expect(argIdx).toBeGreaterThan(-1);
			const instructionsArg = argv[argIdx + 1];
			expect(instructionsArg).toContain(vmLevelInstructions);

			vm.closeSession(sessionId);
		} finally {
			restore();
		}
	});
});
