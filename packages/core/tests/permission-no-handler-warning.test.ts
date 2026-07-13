import { afterEach, describe, expect, test } from "vitest";
import type { PermissionReply } from "../src/index.js";
import { AgentOs } from "../src/index.js";

// ---------------------------------------------------------------------------
// #1542 follow-up — when an agent requests a tool permission but the host has
// registered NO `onPermissionRequest` handler, the client returns no explicit
// answer and the ACP sidecar applies its default. A host that forgot to wire the
// hook would otherwise see only silent denials, so the client still emits one
// host-visible warning per session through the `onAgentStderr` channel.
// ---------------------------------------------------------------------------

interface PendingReply {
	resolve: (reply: PermissionReply) => void;
	reject: (error: Error) => void;
	timer: ReturnType<typeof setTimeout>;
}

function injectSession(vm: AgentOs, sessionId: string): void {
	const sessions = (vm as unknown as { _sessions: Map<string, unknown> })
		._sessions;
	sessions.set(sessionId, {
		sessionId,
		agentType: "mock",
		processId: "",
		pid: null,
		closed: false,
		modes: null,
		configOptions: [],
		capabilities: {},
		agentInfo: null,
		eventHandlers: new Set(),
		permissionHandlers: new Set(),
		warnedNoPermissionHandler: false,
		configOverrides: new Map(),
		pendingPermissionReplies: new Map<string, PendingReply>(),
	});
}

function callPermissionCallback(
	vm: AgentOs,
	sessionId: string,
	permissionId: string,
	params: Record<string, unknown>,
): Promise<PermissionReply | undefined> {
	return (
		vm as unknown as {
			_handleAcpPermissionCallback: (
				sessionId: string,
				permissionId: string,
				params: Record<string, unknown>,
				timeoutMs: number,
			) => Promise<PermissionReply | undefined>;
		}
	)._handleAcpPermissionCallback(sessionId, permissionId, params, 120_000);
}

describe("permission request with no host handler (#1542)", () => {
	let vm: AgentOs | null = null;

	afterEach(async () => {
		await vm?.dispose();
		vm = null;
	});

	test("defers the default to the sidecar and warns once per session", async () => {
		const stderr: string[] = [];
		const decoder = new TextDecoder();
		vm = await AgentOs.create({
			onAgentStderr: (event) => {
				stderr.push(decoder.decode(event.chunk));
			},
		});

		injectSession(vm, "session-A");

		// First unanswered tool request emits exactly one warning. Use the
		// real ACP permission param shape — the sidecar forwards the agent's
		// `{ toolCall: { title } }`, with no top-level `toolName` — so the label is
		// resolved exactly as it is in production.
		await expect(
			callPermissionCallback(vm, "session-A", "1", {
				toolCall: { title: "Bash" },
			}),
		).resolves.toBeUndefined();

		const warnings = () =>
			stderr.filter((line) =>
				line.includes("no onPermissionRequest handler is registered"),
			);
		expect(warnings()).toHaveLength(1);
		// The warning names the tool (from toolCall.title) and the remediation API.
		expect(warnings()[0]).toContain("Bash");
		expect(warnings()[0]).toContain("vm.onPermissionRequest");
		expect(warnings()[0]).toContain("session-A");

		// A second unanswered request in the same session must NOT re-warn.
		await expect(
			callPermissionCallback(vm, "session-A", "2", {
				toolCall: { title: "Edit" },
			}),
		).resolves.toBeUndefined();
		expect(warnings()).toHaveLength(1);
	});

	test("does not warn when a permission handler is registered", async () => {
		const stderr: string[] = [];
		const decoder = new TextDecoder();
		vm = await AgentOs.create({
			onAgentStderr: (event) => {
				stderr.push(decoder.decode(event.chunk));
			},
		});

		injectSession(vm, "session-A");
		// A registered handler that approves immediately keeps the request off the
		// no-handler deny path.
		const reg = vm;
		reg.onPermissionRequest("session-A", (request) => {
			void reg.respondPermission("session-A", request.permissionId, "once");
		});

		await expect(
			callPermissionCallback(vm, "session-A", "1", {
				toolCall: { title: "Bash" },
			}),
		).resolves.toBe("once");

		const warnings = stderr.filter((line) =>
			line.includes("no onPermissionRequest handler is registered"),
		);
		expect(warnings).toHaveLength(0);
		// The once-per-session guard must not have been tripped.
		const session = (
			vm as unknown as {
				_sessions: Map<string, { warnedNoPermissionHandler: boolean }>;
			}
		)._sessions.get("session-A");
		expect(session?.warnedNoPermissionHandler).toBe(false);
	});

	test("a different session warns independently", async () => {
		const stderr: string[] = [];
		const decoder = new TextDecoder();
		vm = await AgentOs.create({
			onAgentStderr: (event) => {
				stderr.push(decoder.decode(event.chunk));
			},
		});

		injectSession(vm, "session-A");
		injectSession(vm, "session-B");

		await callPermissionCallback(vm, "session-A", "1", { toolName: "Bash" });
		await callPermissionCallback(vm, "session-B", "1", { toolName: "Bash" });

		const warnings = stderr.filter((line) =>
			line.includes("no onPermissionRequest handler is registered"),
		);
		expect(warnings).toHaveLength(2);
		expect(warnings.some((w) => w.includes("session-A"))).toBe(true);
		expect(warnings.some((w) => w.includes("session-B"))).toBe(true);
	});
});
