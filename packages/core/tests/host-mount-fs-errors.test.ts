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

const readOnlyMountProgram = `
const fs = require("node:fs");
const shape = (error) => error ? { code: error.code, errno: error.errno, syscall: error.syscall } : null;
const sync = (run) => { try { run(); return null; } catch (error) { return shape(error); } };
const promise = (run) => run().then(() => null, shape);
(async () => {
	process.stdout.write(JSON.stringify({
		read: fs.readFileSync("/ro/file.txt", "utf8"),
		readOnly: {
			writeFileSync: sync(() => fs.writeFileSync("/ro/file.txt", "y")),
			writeFileSyncNew: sync(() => fs.writeFileSync("/ro/new.txt", "y")),
			appendFileSync: sync(() => fs.appendFileSync("/ro/file.txt", "y")),
			openSyncWrite: sync(() => fs.openSync("/ro/file.txt", "r+")),
			openSyncCreate: sync(() => fs.openSync("/ro/created.txt", "wx")),
			truncateSync: sync(() => fs.truncateSync("/ro/file.txt", 0)),
			mkdirSync: sync(() => fs.mkdirSync("/ro/newdir")),
			mkdirSyncRecursive: sync(() => fs.mkdirSync("/ro/a/b", { recursive: true })),
			unlinkSync: sync(() => fs.unlinkSync("/ro/file.txt")),
			renameSync: sync(() => fs.renameSync("/ro/file.txt", "/ro/renamed.txt")),
			rmdirSync: sync(() => fs.rmdirSync("/ro/dir")),
			chmodSync: sync(() => fs.chmodSync("/ro/file.txt", 0o600)),
			utimesSync: sync(() => fs.utimesSync("/ro/file.txt", 1, 1)),
			symlinkSync: sync(() => fs.symlinkSync("file.txt", "/ro/link")),
			linkSync: sync(() => fs.linkSync("/ro/file.txt", "/ro/hard")),
			promisesWriteFile: await promise(() => fs.promises.writeFile("/ro/promised.txt", "y")),
			promisesMkdir: await promise(() => fs.promises.mkdir("/ro/promised")),
		},
		policyDenied: {
			readOnlyMount: sync(() => fs.writeFileSync("/ro/policy.txt", "y")),
			writableMount: sync(() => fs.writeFileSync("/rw/policy.txt", "y")),
		},
		writable: {
			writeFileSync: sync(() => fs.writeFileSync("/rw/new.txt", "written")),
			mkdirSync: sync(() => fs.mkdirSync("/rw/newdir")),
		},
	}));
})();
`;

test("a write to a read-only host mount reports EROFS, as on Linux", async () => {
	const readOnlyDir = fs.mkdtempSync(path.join(os.tmpdir(), "agentos-ro-"));
	const writableDir = fs.mkdtempSync(path.join(os.tmpdir(), "agentos-rw-"));
	fs.writeFileSync(path.join(readOnlyDir, "file.txt"), "x");
	fs.writeFileSync(path.join(readOnlyDir, "policy.txt"), "x");
	fs.mkdirSync(path.join(readOnlyDir, "dir"));
	fs.writeFileSync(path.join(writableDir, "policy.txt"), "x");
	// No write bit on the read-only tree: the guest fails the owner and mode
	// check there, and Linux still reports the read-only filesystem first.
	fs.chmodSync(path.join(readOnlyDir, "file.txt"), 0o444);
	fs.chmodSync(path.join(readOnlyDir, "dir"), 0o555);
	fs.chmodSync(readOnlyDir, 0o555);
	let mounted: AgentOs | undefined;
	try {
		mounted = await AgentOs.create({
			defaultSoftware: false,
			// The guest owns both trees, so a write to the writable mount works.
			user: { uid: process.getuid?.() ?? 0, gid: process.getgid?.() ?? 0 },
			mounts: [
				hostDirMount("/ro", readOnlyDir, { readOnly: true }),
				hostDirMount("/rw", writableDir, { readOnly: false }),
			],
			permissions: {
				fs: {
					default: "allow",
					rules: [
						{
							mode: "deny",
							paths: ["/ro/policy.txt", "/rw/policy.txt"],
							operations: ["write"],
						},
					],
				},
				network: "allow",
				env: "allow",
				process: "allow",
				childProcess: "allow",
				hostFunction: "deny",
			},
		});
		const result = await mounted.process.execFile(
			"node",
			["-e", readOnlyMountProgram],
			{
				timeoutMs: 20_000,
				output: { capture: "all", maxBufferedBytes: 64 * 1024 },
			},
		);
		expect(result.exitCode, result.stderr).toBe(0);
		const readOnly = (syscall: string) => ({
			code: "EROFS",
			errno: -30,
			syscall,
		});
		const denied = { code: "EACCES", errno: -13, syscall: "open" };
		expect(JSON.parse(result.stdout)).toEqual({
			read: "x",
			readOnly: {
				writeFileSync: readOnly("open"),
				writeFileSyncNew: readOnly("open"),
				appendFileSync: readOnly("open"),
				openSyncWrite: readOnly("open"),
				openSyncCreate: readOnly("open"),
				truncateSync: readOnly("truncate"),
				mkdirSync: readOnly("mkdir"),
				mkdirSyncRecursive: readOnly("mkdir"),
				unlinkSync: readOnly("unlink"),
				renameSync: readOnly("rename"),
				rmdirSync: readOnly("rmdir"),
				chmodSync: readOnly("chmod"),
				utimesSync: readOnly("utimes"),
				symlinkSync: readOnly("symlink"),
				linkSync: readOnly("link"),
				promisesWriteFile: readOnly("open"),
				promisesMkdir: readOnly("mkdir"),
			},
			// A permission-policy denial is not a read-only filesystem.
			policyDenied: { readOnlyMount: denied, writableMount: denied },
			writable: { writeFileSync: null, mkdirSync: null },
		});
		expect(fs.readdirSync(readOnlyDir).sort()).toEqual([
			"dir",
			"file.txt",
			"policy.txt",
		]);
		expect(fs.readFileSync(path.join(readOnlyDir, "file.txt"), "utf8")).toBe(
			"x",
		);
		expect(fs.readFileSync(path.join(writableDir, "new.txt"), "utf8")).toBe(
			"written",
		);
	} finally {
		await mounted?.dispose();
		fs.chmodSync(readOnlyDir, 0o755);
		fs.chmodSync(path.join(readOnlyDir, "dir"), 0o755);
		fs.rmSync(readOnlyDir, { recursive: true, force: true });
		fs.rmSync(writableDir, { recursive: true, force: true });
	}
}, 30_000);

// The default guest user does not own the mounted files, so these calls fail
// the ownership check. That check must not hide the read-only filesystem.
test("chmod, chown, and utimes on a read-only host mount report EROFS", async () => {
	const result = await vm.process.execFile(
		"node",
		[
			"-e",
			`
const fs = require("node:fs");
const code = (run) => { try { run(); return null; } catch (error) { return error.code; } };
process.stdout.write(JSON.stringify({
	chmodSync: code(() => fs.chmodSync("/m/file.txt", 0o600)),
	chownSync: code(() => fs.chownSync("/m/file.txt", 0, 0)),
	utimesSync: code(() => fs.utimesSync("/m/file.txt", 1, 1)),
}));
`,
		],
		{
			timeoutMs: 20_000,
			output: { capture: "all", maxBufferedBytes: 64 * 1024 },
		},
	);
	expect(result.exitCode, result.stderr).toBe(0);
	expect(JSON.parse(result.stdout)).toEqual({
		chmodSync: "EROFS",
		chownSync: "EROFS",
		utimesSync: "EROFS",
	});
}, 30_000);
