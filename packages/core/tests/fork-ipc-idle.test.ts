import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let vm: AgentOs;

// A short operation deadline makes an idle channel outlive it quickly. A real
// `next dev` parent and its forked server stay quiet longer than the 31 s
// default after startup. The processes exit explicitly: a separate bug keeps a
// child alive after `disconnect()`.
beforeAll(async () => {
	vm = await AgentOs.create({
		defaultSoftware: false,
		limits: { reactor: { operationDeadlineMs: 1000 } },
	});
});

afterAll(async () => {
	await vm?.dispose();
});

const program = `
const { fork } = require("node:child_process");
const errors = [];
process.on("uncaughtException", (error) => errors.push("parent:" + error.code));
if (process.argv[2] === "child") {
	process.on("uncaughtException", (error) => process.send?.("child:" + error.code));
	process.on("message", (message) =>
		process.send("pong:" + message, () => process.exit(0)),
	);
} else {
	const child = fork(__filename, ["child"]);
	const messages = [];
	child.on("message", (message) => messages.push(message));
	child.on("exit", (code) => {
		process.stdout.write(JSON.stringify({ code, messages, errors }));
		process.exit(0);
	});
	setTimeout(() => child.send("ping"), 2500);
}
`;

test("an idle fork() IPC channel outlives the operation deadline", async () => {
	await vm.filesystem.writeFiles([
		{ path: "/tmp/ipc-idle.cjs", content: program },
	]);
	const result = await vm.process.execFile("node", ["/tmp/ipc-idle.cjs"], {
		timeoutMs: 20_000,
		output: { capture: "all", maxBufferedBytes: 64 * 1024 },
	});
	expect(result.exitCode, result.stderr).toBe(0);
	expect(JSON.parse(result.stdout)).toEqual({
		code: 0,
		messages: ["pong:ping"],
		errors: [],
	});
}, 30_000);
