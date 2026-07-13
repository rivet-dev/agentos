import {
	chmodSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

/**
 * End-to-end proof that an `/opt/agentos` AGENT package launches a session:
 * a package dir with `agentos-package.json` registers an agent whose ACP adapter
 * is the `bin/<acpEntrypoint>` command, and
 * `createSession(name)` spawns it via `/opt/agentos/bin/<acpEntrypoint>` — no
 * npm package resolution. The adapter is a hand-built minimal ACP stdio server.
 */
const MOCK_ACP_ADAPTER = `
let buffer = "";
function writeMessage(message) { process.stdout.write(JSON.stringify(message) + "\\n"); }
function writeResponse(id, result) { writeMessage({ jsonrpc: "2.0", id, result }); }
process.stdin.resume();
process.stdin.on("data", (chunk) => {
  buffer += new TextDecoder().decode(chunk);
  while (true) {
    const nl = buffer.indexOf("\\n");
    if (nl === -1) break;
    const line = buffer.slice(0, nl);
    buffer = buffer.slice(nl + 1);
    if (!line.trim()) continue;
    const msg = JSON.parse(line);
    if (msg.id === undefined) continue;
    switch (msg.method) {
      case "initialize":
        writeResponse(msg.id, {
          protocolVersion: 1,
          agentInfo: { name: "opt-agentos-mock-agent", version: "1.0.0" },
          agentCapabilities: { plan_mode: false, tool_calls: false, promptCapabilities: {} },
          modes: { currentModeId: "default", availableModes: [{ id: "default", label: "Default" }] },
          configOptions: [],
        });
        break;
      case "session/new":
        writeResponse(msg.id, {
          sessionId: "opt-agentos-session-1",
          modes: { currentModeId: "default", availableModes: [{ id: "default", label: "Default" }] },
          configOptions: [],
        });
        break;
      case "session/prompt":
        writeMessage({ jsonrpc: "2.0", method: "session/update", params: {
          sessionId: "opt-agentos-session-1",
          update: { sessionUpdate: "agent_message_chunk", content: { text: "opt-agentos-agent-ok" } } } });
        writeMessage({ jsonrpc: "2.0", method: "session/update", params: {
          sessionId: "opt-agentos-session-1",
          update: { sessionUpdate: "completed", stopReason: "end_turn" } } });
        writeResponse(msg.id, { stopReason: "end_turn" });
        break;
      case "session/cancel":
        writeResponse(msg.id, {});
        break;
      default:
        writeMessage({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "Method not found" } });
        break;
    }
  }
});
`.trim();

describe("agentos agent package (VM)", () => {
	let vm: AgentOs;
	let root: string;

	beforeAll(async () => {
		root = mkdtempSync(join(tmpdir(), "agentos-agent-pkg-"));
		// A self-contained /opt/agentos agent package: bin/<acpEntrypoint> is the
		// ACP adapter, and the manifest names it via the agent block.
		const pkgDir = join(root, "pkg");
		mkdirSync(join(pkgDir, "bin"), { recursive: true });
		writeFileSync(
			join(pkgDir, "package.json"),
			JSON.stringify({ name: "mock-agent", version: "1.0.0" }, null, 2),
		);
		writeFileSync(
			join(pkgDir, "agentos-package.json"),
			JSON.stringify(
				{
					name: "mock-agent",
					version: "1.0.0",
					agent: { acpEntrypoint: "mock-agent-acp" },
				},
				null,
				2,
			),
		);
		const binPath = join(pkgDir, "bin", "mock-agent-acp");
		writeFileSync(binPath, `#!/usr/bin/env node\n${MOCK_ACP_ADAPTER}\n`);
		chmodSync(binPath, 0o755);

		vm = await AgentOs.create({
			defaultSoftware: false,
			software: [pkgDir],
		});
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
		if (root) rmSync(root, { recursive: true, force: true });
	});

	test("lists the packaged agent as installed", async () => {
		// listAgents() is a sidecar ACP RPC: the sidecar enumerates the projected
		// `/opt/agentos` packages. The entry is just id + installed (no client-side
		// entrypoint resolution).
		const agents = await vm.listAgents();
		const entry = agents.find((a) => a.id === "mock-agent");
		expect(entry).toBeDefined();
		expect(entry?.installed).toBe(true);
	});

	test("createSession launches the packaged agent via /opt/agentos/bin", async () => {
		const session = await vm.createSession("mock-agent");
		expect(session.sessionId).toBeTruthy();
		expect(await vm.listSessions()).toContainEqual({
			sessionId: session.sessionId,
			agentType: "mock-agent",
		});
		await vm.closeSession(session.sessionId);
		expect(await vm.listSessions()).not.toContainEqual({
			sessionId: session.sessionId,
			agentType: "mock-agent",
		});
		await expect(vm.closeSession(session.sessionId)).resolves.toBeUndefined();
		await expect(vm.closeSession("never-created")).resolves.toBeUndefined();
	});
});
