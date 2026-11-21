import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>;
let vm: AgentOs;

beforeAll(async () => {
	sidecar = await AgentOs.createSidecar();
	vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
		defaultSoftware: false,
		permissions: {
			fs: "allow",
			process: "allow",
			childProcess: "allow",
			network: "deny",
		},
	});
});

afterAll(async () => {
	try {
		await vm?.dispose();
	} finally {
		await sidecar?.dispose();
	}
});

// `next build` (TypeScript) runs `tsc --showConfig`, awaits promise work, then
// forks its build worker. The fork must keep the parent alive after the first
// child's handle drained.
const worker = `setTimeout(() => process.send("done"), 500);`;
const parent = `
const cp = require("node:child_process");
const fs = require("node:fs");
const first = cp.spawn(process.execPath, ["-e", "0"], { stdio: "ignore" });
first.on("exit", () => {
	fs.promises.readFile("/tmp/worker.cjs").then(() => {
		const child = cp.fork("/tmp/worker.cjs", [], { silent: true });
		child.on("message", (message) => {
			process.stdout.write(String(message));
			child.kill();
		});
	});
});
`;

test("a child forked after the first handle drain keeps its parent alive until it replies", async () => {
	await vm.filesystem.writeFiles([
		{ path: "/tmp/worker.cjs", content: worker },
	]);
	const result = await vm.process.execFile("node", ["-e", parent], {
		timeoutMs: 20_000,
		output: { capture: "all", maxBufferedBytes: 64 * 1024 },
	});
	expect(result.exitCode, result.stderr).toBe(0);
	expect(result.stdout).toBe("done");
}, 30_000);
