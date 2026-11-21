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

// Go js/wasm (esbuild-wasm) reads stdin only through a callback `fs.read(0)`.
// That pending read is the child's only work while the parent stays quiet.
const child = `
const fs = require("node:fs");
const buffer = Buffer.alloc(64);
fs.read(0, buffer, 0, 64, null, (error, bytesRead) => {
	process.stdout.write(error ? "error:" + error.code : "read:" + buffer.toString("utf8", 0, bytesRead));
});
`;

const parent = `
const { spawn } = require("node:child_process");
const child = spawn(process.execPath, ["-e", ${JSON.stringify(child)}], { stdio: ["pipe", "pipe", "inherit"] });
let output = "";
child.stdout.on("data", (chunk) => { output += chunk; });
child.on("close", (code) => process.stdout.write(JSON.stringify({ code, output })));
setTimeout(() => {
	child.stdin.write("ping");
	setTimeout(() => child.stdin.end(), 200);
}, 1500);
`;

test("a child whose only work is a pending fs.read(0, cb) receives data written later", async () => {
	const result = await vm.process.execFile("node", ["-e", parent], {
		timeoutMs: 20_000,
		output: { capture: "all", maxBufferedBytes: 64 * 1024 },
	});
	expect(result.exitCode, result.stderr).toBe(0);
	expect(JSON.parse(result.stdout)).toEqual({ code: 0, output: "read:ping" });
}, 30_000);
