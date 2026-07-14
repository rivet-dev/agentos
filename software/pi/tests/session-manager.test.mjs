import test from "node:test";
import assert from "node:assert/strict";
import { resolve as resolvePath } from "node:path";

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

test("PI_SESSION_DIR persists and resumes via continueRecent", () => {
	const manager = fakeSessionManager();
	const result = resolveSessionManager(manager, "/workspace", {
		PI_SESSION_DIR: "/sessions/a/main",
	});
	assert.deepEqual(manager.calls, [
		["continueRecent", "/workspace", "/sessions/a/main"],
	]);
	assert.equal(result.kind, "continueRecent");
});

test("an omitted PI_SESSION_DIR keeps the in-memory default", () => {
	const manager = fakeSessionManager();
	const result = resolveSessionManager(manager, "/workspace", {});
	assert.deepEqual(manager.calls, [["inMemory", "/workspace"]]);
	assert.equal(result.kind, "inMemory");
});

test("a blank PI_SESSION_DIR is ignored", () => {
	const manager = fakeSessionManager();
	resolveSessionManager(manager, "/workspace", { PI_SESSION_DIR: "   " });
	assert.deepEqual(manager.calls, [["inMemory", "/workspace"]]);
});
