import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { afterAll, beforeAll, expect, test } from "vitest";
import { AgentOs, hostDirMount } from "../src/index.js";

let hostDir: string;
let vm: AgentOs;

beforeAll(async () => {
	hostDir = fs.mkdtempSync(path.join(os.tmpdir(), "agentos-mount-errors-"));
	// mkdtemp creates 0700; the guest user must be able to search the mount.
	fs.chmodSync(hostDir, 0o755);
	fs.writeFileSync(path.join(hostDir, "file.txt"), "x");
	vm = await AgentOs.create({
		defaultSoftware: false,
		mounts: [hostDirMount("/m", hostDir, { readOnly: true })],
	});
});

afterAll(async () => {
	await vm?.dispose();
	fs.rmSync(hostDir, { recursive: true, force: true });
});

// webpack's resolver and similar tools branch on `err.code`; every failure must
// carry the Node fs error shape, whichever API form raised it.
const program = `
const fs = require("node:fs");
const shape = (error) => error ? { code: error.code, errno: error.errno, syscall: error.syscall, path: error.path } : null;
const sync = (run) => { try { run(); return null; } catch (error) { return shape(error); } };
const callback = (run) => new Promise((resolve) => run((error) => resolve(shape(error))));
const promise = (run) => run().then(() => null, shape);
(async () => {
	const notDir = "/m/file.txt/child";
	const missing = "/m/missing.txt";
	process.stdout.write(JSON.stringify({
		readFileSync: sync(() => fs.readFileSync(notDir)),
		statSync: sync(() => fs.statSync(notDir)),
		readdirFile: sync(() => fs.readdirSync("/m/file.txt")),
		promisesStat: await promise(() => fs.promises.stat(notDir)),
		promisesReadFile: await promise(() => fs.promises.readFile(notDir)),
		realpath: await callback((cb) => fs.realpath(notDir, cb)),
		missingReadFileSync: sync(() => fs.readFileSync(missing)),
		missingLstat: await promise(() => fs.promises.lstat(missing)),
	}));
})();
`;

test("fs errors under a host mount carry the POSIX code, errno, syscall, and path", async () => {
	const result = await vm.process.execFile("node", ["-e", program], {
		timeoutMs: 20_000,
		output: { capture: "all", maxBufferedBytes: 64 * 1024 },
	});
	expect(result.exitCode, result.stderr).toBe(0);
	const notDir = (syscall: string) => ({
		code: "ENOTDIR",
		errno: -20,
		syscall,
		path: "/m/file.txt/child",
	});
	const missing = (syscall: string) => ({
		code: "ENOENT",
		errno: -2,
		syscall,
		path: "/m/missing.txt",
	});
	expect(JSON.parse(result.stdout)).toEqual({
		readFileSync: notDir("open"),
		statSync: notDir("stat"),
		readdirFile: {
			code: "ENOTDIR",
			errno: -20,
			syscall: "scandir",
			path: "/m/file.txt",
		},
		promisesStat: notDir("stat"),
		promisesReadFile: notDir("open"),
		realpath: notDir("realpath"),
		missingReadFileSync: missing("open"),
		missingLstat: missing("lstat"),
	});
}, 30_000);
