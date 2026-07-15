import test from "node:test";
import assert from "node:assert/strict";
import { resolve as resolvePath } from "node:path";

// Unit tests for Pi session manager selection. The real session path needs the
// Pi SDK, so we test the persistence and exact-load behavior with a fake.
const packageDir = resolvePath(import.meta.dirname, "..");
const { createSessionManager, loadSessionManager } = await import(
	resolvePath(packageDir, "dist", "adapter.js")
);

function fakeSessionManager() {
	const calls = [];
	return {
		calls,
		create(cwd, dir) {
			calls.push(["create", cwd, dir]);
			return { kind: "create", cwd, dir };
		},
		open(path, dir) {
			calls.push(["open", path, dir]);
			return { kind: "open", path, dir };
		},
		async list(cwd, dir) {
			calls.push(["list", cwd, dir]);
			return [
				{ id: "session-a", path: "/sessions/a.jsonl" },
				{ id: "session-b", path: "/sessions/b.jsonl" },
			];
		},
	};
}

test("new sessions persist in PI_SESSION_DIR without resuming an earlier session", () => {
	const sm = fakeSessionManager();
	const result = createSessionManager(sm, "/workspace", {
		PI_SESSION_DIR: "/sessions/a/main",
	});
	assert.deepEqual(sm.calls, [["create", "/workspace", "/sessions/a/main"]]);
	assert.equal(result.kind, "create");
});

test("new sessions persist in Pi's default directory when PI_SESSION_DIR is unset", () => {
	const sm = fakeSessionManager();
	const result = createSessionManager(sm, "/workspace", {});
	assert.deepEqual(sm.calls, [["create", "/workspace", undefined]]);
	assert.equal(result.kind, "create");
});

test("blank PI_SESSION_DIR uses Pi's default persisted directory", () => {
	const sm = fakeSessionManager();
	createSessionManager(sm, "/workspace", { PI_SESSION_DIR: "   " });
	assert.deepEqual(sm.calls, [["create", "/workspace", undefined]]);
});

test("session/load opens the exact persisted Pi session", async () => {
	const sm = fakeSessionManager();
	const result = await loadSessionManager(sm, "/workspace", "session-b", {
		PI_SESSION_DIR: "/sessions",
	});
	assert.deepEqual(sm.calls, [
		["list", "/workspace", "/sessions"],
		["open", "/sessions/b.jsonl", "/sessions"],
	]);
	assert.equal(result.kind, "open");
});

test("missing session/load returns the typed fallback sentinel", async () => {
	const sm = fakeSessionManager();
	await assert.rejects(
		loadSessionManager(sm, "/workspace", "missing", {}),
		(error) =>
			error?.code === -32602 && error?.data?.kind === "unknown_session",
	);
});
