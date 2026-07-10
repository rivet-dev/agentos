import test, { after } from "node:test";
import assert from "node:assert/strict";
import { resolve as resolvePath } from "node:path";

// Stop fns for every "open" fake query, drained after the run so the dangling
// consume() loops (and the process) can exit.
const cleanups = [];
after(() => {
	for (const stop of cleanups) stop();
});

// Unit tests for the ClaudeQuerySession adapter fixes. The class is exported
// for testing and its constructor takes an injectable `queryFactory`, so these
// drive the translation/permission/teardown logic with a fake Claude SDK query
// and a mock ACP connection — no real SDK, no VM.
const packageDir = resolvePath(import.meta.dirname, "..");
const { ClaudeQuerySession } = await import(
	resolvePath(packageDir, "dist", "adapter.js")
);

function makeConn(overrides = {}) {
	let closeConn;
	const closed = new Promise((r) => {
		closeConn = r;
	});
	const updates = [];
	return {
		updates,
		closeConn: () => closeConn(),
		sessionUpdate: async (u) => {
			updates.push(u);
		},
		requestPermission: async () => ({
			outcome: { outcome: "selected", optionId: "allow_once" },
		}),
		closed,
		...overrides,
	};
}

function makeQuery({ endImmediately = false } = {}) {
	let stop;
	const stopped = new Promise((r) => {
		stop = r;
	});
	if (!endImmediately) cleanups.push(stop);
	return {
		setMcpServers: async () => {},
		interrupt: async () => {},
		setPermissionMode: async () => {},
		async *[Symbol.asyncIterator]() {
			if (endImmediately) return;
			await stopped; // ends consume() when drained in `after`
		},
	};
}

function makeSession({ endImmediately = false, conn } = {}) {
	const c = conn ?? makeConn();
	let capturedOptions;
	const queryFactory = (arg) => {
		capturedOptions = arg.options;
		return makeQuery({ endImmediately });
	};
	const sess = new ClaudeQuerySession(
		c,
		"sess-1",
		"/workspace",
		"default",
		{ cwd: "/workspace", mcpServers: undefined },
		"/usr/bin/claude",
		queryFactory,
	);
	return { sess, conn: c, getOptions: () => capturedOptions };
}

// ── Fix #5: input_json_delta maps to the correct tool by content-block index ──
test("claude #5: partial tool input is attributed by content-block index, not insertion order", async () => {
	const { sess, conn } = makeSession();
	sess.pendingTurn = {
		sawAssistantText: false,
		sawToolCall: false,
		resolve() {},
		reject() {},
	};
	// A text block occupies content-block index 0; the tool_use block is index 1.
	await sess.handleStreamEvent({
		event: { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "thinking..." } },
	});
	await sess.handleStreamEvent({
		event: {
			type: "content_block_start",
			index: 1,
			content_block: { type: "tool_use", id: "tool-A", name: "Bash", input: {} },
		},
	});
	await sess.handleStreamEvent({
		event: { type: "content_block_delta", index: 1, delta: { type: "input_json_delta", partial_json: '{"command":"ls"}' } },
	});
	await sess.lastEmit;

	const toolUpdates = conn.updates
		.map((u) => u.update)
		.filter((u) => u?.sessionUpdate === "tool_call_update");
	// With the old insertion-order lookup, findToolCallByIndex(1) on a 1-entry
	// map returns null and the partial input is silently dropped (no update).
	assert.equal(toolUpdates.length, 1, "expected exactly one tool_call_update for the partial input");
	assert.equal(toolUpdates[0].toolCallId, "tool-A", "partial input must target the tool at block index 1");
	assert.equal(toolUpdates[0].rawInput.partial_json, '{"command":"ls"}');
});

test("claude #5: two tool_use blocks at different indices each get their own partial input", async () => {
	const { sess, conn } = makeSession();
	sess.pendingTurn = { sawAssistantText: false, sawToolCall: false, resolve() {}, reject() {} };
	// index 0 = text, index 1 = toolA, index 2 = toolB
	await sess.handleStreamEvent({ event: { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: "x" } } });
	await sess.handleStreamEvent({ event: { type: "content_block_start", index: 1, content_block: { type: "tool_use", id: "tool-A", name: "Bash", input: {} } } });
	await sess.handleStreamEvent({ event: { type: "content_block_start", index: 2, content_block: { type: "tool_use", id: "tool-B", name: "Read", input: {} } } });
	await sess.handleStreamEvent({ event: { type: "content_block_delta", index: 2, delta: { type: "input_json_delta", partial_json: '{"path":"/a"}' } } });
	await sess.handleStreamEvent({ event: { type: "content_block_delta", index: 1, delta: { type: "input_json_delta", partial_json: '{"command":"echo"}' } } });
	await sess.lastEmit;

	const updates = conn.updates.map((u) => u.update).filter((u) => u?.sessionUpdate === "tool_call_update");
	const byTool = Object.fromEntries(updates.map((u) => [u.toolCallId, u.rawInput.partial_json]));
	assert.equal(byTool["tool-B"], '{"path":"/a"}', "block index 2 → tool-B");
	assert.equal(byTool["tool-A"], '{"command":"echo"}', "block index 1 → tool-A");
});

// ── Fix #1: permission handler is host-authoritative; no timer auto-resolve ──
test("claude #1: permission handler returns the host's deny decision", async () => {
	const conn = makeConn({
		requestPermission: async () => ({ outcome: { outcome: "selected", optionId: "reject_once" } }),
	});
	const { getOptions } = makeSession({ conn });
	const canUseTool = getOptions().canUseTool;
	const result = await canUseTool("Bash", { command: "rm -rf /" }, { toolUseID: "t1", title: "Bash", suggestions: [] });
	assert.equal(result.behavior, "deny", "host reject must produce a deny");
});

test("claude #1: permission handler does NOT auto-resolve on a timer when the host is silent", async () => {
	let pendingResolve;
	const conn = makeConn({
		// Never settles — simulates a host that hasn't answered yet.
		requestPermission: () => new Promise((r) => {
			pendingResolve = r;
		}),
	});
	const { getOptions } = makeSession({ conn });
	const canUseTool = getOptions().canUseTool;
	const handlerPromise = canUseTool("Bash", {}, { toolUseID: "t1", title: "Bash", suggestions: [] });
	const timer = new Promise((r) => setTimeout(() => r("TIMER_WON"), 300));
	const winner = await Promise.race([handlerPromise.then(() => "HANDLER_WON"), timer]);
	assert.equal(winner, "TIMER_WON", "handler must not auto-resolve before the host answers (no fail-open timer)");
	// settle the pending request so the handler promise doesn't dangle
	pendingResolve({ outcome: { outcome: "selected", optionId: "reject_once" } });
	await handlerPromise;
});

// ── Fix #2: emit logs delivery failures (host-visible) and keeps the chain alive ──
test("claude #2: a failed sessionUpdate is logged to stderr and the emit chain survives", async () => {
	let failNext = true;
	const delivered = [];
	const conn = makeConn({
		sessionUpdate: async (u) => {
			if (failNext) {
				failNext = false;
				throw new Error("broken pipe");
			}
			delivered.push(u);
		},
	});
	const { sess } = makeSession({ conn });

	const writes = [];
	const orig = process.stderr.write;
	process.stderr.write = (s) => {
		writes.push(String(s));
		return true;
	};
	try {
		await sess.emit({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "a" } });
		await sess.emit({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "b" } });
		await sess.lastEmit;
	} finally {
		process.stderr.write = orig;
	}
	assert.ok(
		writes.some((w) => w.includes("failed to deliver session/update")),
		"a delivery failure must be written to stderr, not swallowed",
	);
	assert.equal(delivered.length, 1, "the chain must survive the failure and deliver the next update");
});

// ── Fix #4: a dead reader marks the session closed so prompt() fails fast ──
test("claude #4: once the query stream ends, prompt() fails fast instead of hanging", async () => {
	const { sess } = makeSession({ endImmediately: true });
	await sess.reader; // wait for consume() to finish on the now-ended query
	await assert.rejects(
		sess.prompt({ prompt: [{ type: "text", text: "hi" }] }),
		/Session is closed/,
		"a prompt on a dead session must reject promptly, not hang",
	);
});
