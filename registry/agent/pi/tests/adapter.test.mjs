import test from "node:test";
import assert from "node:assert/strict";
import { resolve as resolvePath } from "node:path";

// Unit tests for the PiSdkAgent adapter fixes. The class is exported for
// testing; newSession needs the real Pi SDK, so these drive the translation /
// lifecycle logic directly with a mock ACP connection + a fake session.
const packageDir = resolvePath(import.meta.dirname, "..");
const { PiSdkAgent, buildDefaultResourceLoaderOptions } = await import(
	resolvePath(packageDir, "dist", "adapter.js")
);

function makeConn(overrides = {}) {
	return {
		closed: new Promise(() => {}), // never closes unless a test overrides
		sessionUpdate: async () => {},
		...overrides,
	};
}

function fakeSession(overrides = {}) {
	return {
		messages: [],
		prompt: async () => {},
		abort: async () => {},
		subscribe: () => () => {},
		...overrides,
	};
}

// ── Fix #1650: appended system prompt text must not be treated as a path ──
test("pi #1650: DefaultResourceLoader receives appended prompt via override, not path source", () => {
	const appendSystemPrompt = "# agentOS\n\n".repeat(200);
	const extensionFactory = () => {};
	const options = buildDefaultResourceLoaderOptions({
		cwd: "/home/agentos/workspace",
		agentDir: "/home/agentos/.pi/agent",
		extensionFactories: [extensionFactory],
		appendSystemPrompt,
	});

	assert.equal(
		Object.hasOwn(options, "appendSystemPrompt"),
		false,
		"long prompt text must not be passed to Pi's path-probing appendSystemPrompt option",
	);
	assert.equal(typeof options.appendSystemPromptOverride, "function");
	assert.deepEqual(options.appendSystemPromptOverride(["from-file"]), [
		"from-file",
		appendSystemPrompt,
	]);
	assert.deepEqual(options.extensionFactories, [extensionFactory]);
	assert.equal(options.noExtensions, true);
});

// ── Fix #3: editSnapshots is cleared each turn (no unbounded leak) ──
test("pi #3: prompt() clears leaked edit snapshots from a prior aborted turn", async () => {
	const agent = new PiSdkAgent(makeConn());
	agent.session = fakeSession();
	agent.sessionId = "s1";
	// Simulate a tool that started an edit but never reached tool_execution_end.
	agent.editSnapshots.set("tool-1", { path: "/workspace/a.txt", oldText: "before" });
	assert.equal(agent.editSnapshots.size, 1);

	await agent.prompt({ prompt: [{ type: "text", text: "hi" }] });
	assert.equal(agent.editSnapshots.size, 0, "editSnapshots must be reset at prompt start");
});

test("pi #3: cancel() clears the per-turn tool maps", async () => {
	const agent = new PiSdkAgent(makeConn());
	agent.session = fakeSession();
	agent.editSnapshots.set("tool-1", { path: "/a", oldText: "x" });
	agent.currentToolCalls.set("tool-1", "call-1");
	await agent.cancel({});
	assert.equal(agent.editSnapshots.size, 0);
	assert.equal(agent.currentToolCalls.size, 0);
});

// ── Fix #4: the live subscription is disposed when the connection closes ──
test("pi #4: connection close disposes the session subscription", async () => {
	let closeConn;
	const conn = makeConn({ closed: new Promise((r) => (closeConn = r)) });
	const agent = new PiSdkAgent(conn);
	let unsubscribed = false;
	agent.unsubscribe = () => {
		unsubscribed = true;
	};
	agent.session = fakeSession();

	closeConn();
	await new Promise((r) => setTimeout(r, 10)); // let conn.closed.then(dispose) run

	assert.ok(unsubscribed, "conn.closed must call the subscription disposer");
	assert.equal(agent.session, null, "the session reference must be dropped on dispose");
});

// ── Fix #2: emit logs delivery failures (host-visible) and survives them ──
test("pi #2: a failed sessionUpdate is logged and the emit chain survives", async () => {
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
	const agent = new PiSdkAgent(conn);
	agent.sessionId = "s1";

	const warns = [];
	const origWarn = console.warn;
	console.warn = (...a) => warns.push(a.join(" "));
	try {
		await agent.sendUpdate({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "a" } });
		await agent.sendUpdate({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: "b" } });
		await agent.lastEmit;
	} finally {
		console.warn = origWarn;
	}

	assert.ok(
		warns.some((w) => w.includes("failed to deliver session/update")),
		"a delivery failure must be logged, not swallowed",
	);
	assert.equal(delivered.length, 1, "the chain must survive the failure and deliver the next update");
});
