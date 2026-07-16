import { resolve } from "node:path";
import { moduleAccessMounts } from "./helpers/node-modules-mount.js";
import { describe, expect, test } from "vitest";
import { AgentOs, type AgentInfo } from "../src/agent-os.js";
import { createProjectedAgentPackage } from "./helpers/projected-agent-package.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");
const CAPTURED_ENV_KEYS = [
	"PI_ACP_PI_COMMAND",
	"CLAUDE_CODE_DISABLE_CWD_PERSIST",
	"CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT",
	"CLAUDE_CODE_NODE_SHELL_WRAPPER",
	"CLAUDE_CODE_SHELL",
	"CLAUDE_CODE_SIMPLE_SHELL_EXEC",
	"CLAUDE_CODE_SWAP_STDIO",
	"SHELL",
	"ACP_APPEND_SYSTEM_PROMPT",
	"OPENCODE_CONTEXTPATHS",
] as const;

const MOCK_ACP_ADAPTER = `
const capturedEnvKeys = ${JSON.stringify(CAPTURED_ENV_KEYS)};
let buffer = "";

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

    let result;
    switch (msg.method) {
      case "initialize":
        result = {
          protocolVersion: 1,
          agentInfo: {
            name: "mock-adapter",
            version: "1.0.0",
            argv: process.argv.slice(2),
            env: Object.fromEntries(
              capturedEnvKeys.map((key) => [key, process.env[key] ?? null]),
            ),
          },
        };
        break;
      case "session/new":
      case "session/cancel":
        result = msg.method === "session/new" ? { sessionId: "mock-session-1" } : {};
        break;
      default:
        process.stdout.write(JSON.stringify({
          jsonrpc: "2.0",
          id: msg.id,
          error: { code: -32601, message: "Method not found" },
        }) + "\\n");
        continue;
    }

    process.stdout.write(JSON.stringify({
      jsonrpc: "2.0",
      id: msg.id,
      result,
    }) + "\\n");
  }
});
`;

type LaunchProbe = AgentInfo & {
	argv?: string[];
	env?: Partial<Record<(typeof CAPTURED_ENV_KEYS)[number], string | null>>;
};

async function inspectLaunch(
	agentType: string,
	agentManifest: {
		env?: Record<string, string>;
		launchArgs?: string[];
	} = {},
): Promise<LaunchProbe> {
	const agentPackage = createProjectedAgentPackage({
		name: agentType,
		adapterScript: MOCK_ACP_ADAPTER,
		...agentManifest,
	});
	const vm = await AgentOs.create({
		defaultSoftware: false,
		mounts: moduleAccessMounts(MODULE_ACCESS_CWD),
		software: [agentPackage.software],
	});
	let sessionId: string | undefined;

	try {
		sessionId = (await vm.createSession(agentType)).sessionId;
		return vm.getSessionAgentInfo(sessionId) as LaunchProbe;
	} finally {
		if (sessionId) {
			vm.closeSession(sessionId);
		}
		await vm.dispose();
		agentPackage.cleanup();
	}
}

describe("agent launch args and env", () => {
	test("Pi SDK receives the adapter-neutral system prompt contract", async () => {
		// The pre-packed pi-sdk adapter embeds the SDK, so (unlike the CLI adapter)
		// it does NOT need a resolved `PI_ACP_PI_COMMAND` pi binary.
		const agentInfo = await inspectLaunch("pi");

		expect(agentInfo.env?.ACP_APPEND_SYSTEM_PROMPT).toContain("# agentOS");
	});

	test("Pi CLI receives the system prompt contract and resolved pi binary", async () => {
		// pi-cli is still the legacy two-package CLI adapter that spawns the pi CLI
		// via PI_ACP_PI_COMMAND.
		const agentInfo = await inspectLaunch("pi-cli", {
			env: { PI_ACP_PI_COMMAND: "pi" },
		});

		expect(agentInfo.env?.ACP_APPEND_SYSTEM_PROMPT).toContain("# agentOS");
		// The {name,dir} model projects the pi CLI onto $PATH as /opt/agentos/bin/pi,
		// so pi-cli points PI_ACP_PI_COMMAND at the projected command name (not a host
		// node_modules path like the old @mariozechner/pi-coding-agent entry).
		expect(agentInfo.env?.PI_ACP_PI_COMMAND).toBe("pi");
	});

	test("Claude injects shell-safe launch env defaults", async () => {
		const agentInfo = await inspectLaunch("claude", {
			env: {
				CLAUDE_CODE_DISABLE_CWD_PERSIST: "1",
				CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT: "1",
				CLAUDE_CODE_NODE_SHELL_WRAPPER: "1",
				CLAUDE_CODE_SHELL: "/bin/sh",
				CLAUDE_CODE_SIMPLE_SHELL_EXEC: "1",
				CLAUDE_CODE_SWAP_STDIO: "0",
				SHELL: "/bin/sh",
			},
		});

		expect(agentInfo.env?.ACP_APPEND_SYSTEM_PROMPT).toContain("# agentOS");
		expect(agentInfo.env).toMatchObject({
			CLAUDE_CODE_DISABLE_CWD_PERSIST: "1",
			CLAUDE_CODE_DISABLE_DEV_NULL_REDIRECT: "1",
			CLAUDE_CODE_NODE_SHELL_WRAPPER: "1",
			CLAUDE_CODE_SHELL: "/bin/sh",
			CLAUDE_CODE_SIMPLE_SHELL_EXEC: "1",
			CLAUDE_CODE_SWAP_STDIO: "0",
			SHELL: "/bin/sh",
		});
	});

	test("OpenCode receives the adapter-neutral system prompt contract", async () => {
		const agentInfo = await inspectLaunch("opencode");

		expect(agentInfo.argv ?? []).not.toContain("--append-system-prompt");
		expect(agentInfo.env?.ACP_APPEND_SYSTEM_PROMPT).toContain("# agentOS");
		expect(agentInfo.env?.OPENCODE_CONTEXTPATHS).toBeNull();
	});

});
