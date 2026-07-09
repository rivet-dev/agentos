import test from "node:test";
import assert from "node:assert/strict";
import { resolve as resolvePath } from "node:path";

// Unit tests for resolveSessionManager. The real newSession path needs the Pi
// SDK, so we test the SessionManager selection directly with a fake.
const packageDir = resolvePath(import.meta.dirname, "..");
const { resolveSessionManager } = await import(
	resolvePath(packageDir, "dist", "adapter.js")
);

function fakeSessionManager() {
	const calls = [];
	return {
		calls,
		inMemory(cwd) {
			calls.push(["inMemory", cwd]);
			return { kind: "inMemory", cwd };
		},
		continueRecent(cwd, dir) {
			calls.push(["continueRecent", cwd, dir]);
			return { kind: "continueRecent", cwd, dir };
		},
	};
}

test("PI_SESSION_DIR persists + resumes via continueRecent", () => {
	const sm = fakeSessionManager();
	const result = resolveSessionManager(sm, "/workspace", {
		PI_SESSION_DIR: "/sessions/a/main",
	});
	assert.deepEqual(sm.calls, [["continueRecent", "/workspace", "/sessions/a/main"]]);
	assert.equal(result.kind, "continueRecent");
});

test("no PI_SESSION_DIR falls back to in-memory (default, unchanged)", () => {
	const sm = fakeSessionManager();
	const result = resolveSessionManager(sm, "/workspace", {});
	assert.deepEqual(sm.calls, [["inMemory", "/workspace"]]);
	assert.equal(result.kind, "inMemory");
});

test("blank/whitespace PI_SESSION_DIR is ignored", () => {
	const sm = fakeSessionManager();
	resolveSessionManager(sm, "/workspace", { PI_SESSION_DIR: "   " });
	assert.deepEqual(sm.calls, [["inMemory", "/workspace"]]);
});
