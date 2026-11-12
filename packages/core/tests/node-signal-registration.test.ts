import { readFileSync } from "node:fs";
import { constants } from "node:os";
import { describe, expect, it } from "vitest";

const source = readFileSync(
	new URL("../../build-tools/bridge-src/builtins/process.ts", import.meta.url),
	"utf8",
);
const signalTable = source.slice(
	source.indexOf("var _signalNumbers ="),
	source.indexOf("var _processKillErrnoByCode ="),
);
const registration = source.slice(
	source.indexOf("function _syncGuestProcessSignalState("),
	source.indexOf("function _syncAllGuestProcessSignalStates("),
);

function adapter(publish: (...args: unknown[]) => void) {
	return new Function(
		"_listenerCountForEvent",
		"_processSignalState",
		`${signalTable}\n${registration}\nreturn _syncGuestProcessSignalState;`,
	)(() => 1, { applySyncPromise: publish });
}

describe("Node signal registration", () => {
	it("rolls back a listener whose kernel registration fails", () => {
		const failure = new Error("registration denied");
		const addSource = source.slice(
			source.indexOf("function _addListener("),
			source.indexOf("function _removeListener("),
		);
		const control = new Function(
			"_syncGuestProcessSignalState",
			`
const _processListeners = {}, _processOnceListeners = {}, _processMaxListeners = 0;
const process2 = {};
${addSource}
return { add: _addListener, listeners: _processListeners };
`,
		)(() => {
			throw failure;
		});
		expect(() => control.add("SIGINT", () => {})).toThrow(failure);
		expect(control.listeners.SIGINT ?? []).toHaveLength(0);
	});

	it("publishes every canonical catchable Linux signal in the supported range", () => {
		const calls: unknown[][] = [];
		const register = adapter((...args) => {
			calls.push(args);
		});
		for (const [name, number] of Object.entries(constants.signals)) {
			if (
				number > 31 ||
				["SIGKILL", "SIGSTOP", "SIGIOT", "SIGPOLL"].includes(name)
			)
				continue;
			calls.length = 0;
			register(name);
			expect(calls, name).toEqual([[undefined, [number, "user", "[]", 0]]]);
		}
	});

	it("preserves a failed kernel registration instead of claiming success", () => {
		const failure = Object.assign(new Error("signal registration failed"), {
			code: "EIO",
		});
		const register = adapter(() => {
			throw failure;
		});
		expect(() => register("SIGINT")).toThrow(failure);
	});
});
