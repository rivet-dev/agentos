import { afterEach, describe, expect, test } from "vitest";
import { AgentOs, createSnapshotExport } from "../src/index.js";

// A synthetic command whose _start calls WASI proc_exit(23).
const nameBytes = (value: string) => [value.length, ...Buffer.from(value)];
const section = (id: number, bytes: number[]) => [id, bytes.length, ...bytes];
const command = Buffer.from([
	0,
	97,
	115,
	109,
	1,
	0,
	0,
	0,
	...section(1, [2, 96, 1, 127, 0, 96, 0, 0]),
	...section(2, [
		1,
		...nameBytes("wasi_snapshot_preview1"),
		...nameBytes("proc_exit"),
		0,
		0,
	]),
	...section(3, [1, 1]),
	...section(5, [1, 0, 1]),
	...section(7, [
		2,
		...nameBytes("_start"),
		0,
		1,
		...nameBytes("memory"),
		2,
		0,
	]),
	...section(10, [1, 6, 0, 65, 23, 16, 0, 11]),
]);

const paths = [
	"/inputs/private.wasm",
	"./private.wasm",
	"/inputs/link.wasm",
	"/inputs/closed/public.wasm",
	"/inputs/closed/link.wasm",
];

describe("child executable authorization", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	async function createVm(denyInputRead = false) {
		vm = await AgentOs.create({
			defaultSoftware: false,
			user: { uid: 0, gid: 0 },
			permissions: {
				fs: denyInputRead
					? {
							default: "allow",
							rules: [
								{
									mode: "deny",
									operations: ["read"],
									paths: ["/inputs/readable.js"],
								},
							],
						}
					: "allow",
				childProcess: "allow",
				process: "allow",
			},
			limits: { resources: { maxProcesses: 4 } },
			rootFilesystem: {
				lowers: [
					createSnapshotExport([
						{
							path: "/inputs",
							type: "directory",
							mode: "0755",
							uid: 0,
							gid: 0,
						},
						{
							path: "/inputs/closed",
							type: "directory",
							mode: "0700",
							uid: 0,
							gid: 0,
						},
						...(
							[
								["public.wasm", "0555"],
								["private.wasm", "0700"],
								["execute-only.wasm", "0111"],
								["closed/public.wasm", "0555"],
							] as const
						).map(([name, mode]) => ({
							path: `/inputs/${name}`,
							type: "file" as const,
							mode,
							uid: 0,
							gid: 0,
							content: command.toString("base64"),
							encoding: "base64" as const,
						})),
						{
							path: "/inputs/link.wasm",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/private.wasm",
						},
						{
							path: "/inputs/closed/link.wasm",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/public.wasm",
						},
						...(
							[
								["public.js", "0755"],
								["private.js", "0704"],
								["readable.js", "0644"],
							] as const
						).map(([name, mode]) => ({
							path: `/inputs/${name}`,
							type: "file" as const,
							mode,
							uid: 0,
							gid: 0,
							content: '#!/usr/bin/env node\nconsole.log("script-executed")',
							encoding: "utf8" as const,
						})),
						{
							path: "/inputs/allowed-input-link.js",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/public.js",
						},
						{
							path: "/inputs/input-link.js",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/readable.js",
						},
						{
							path: "/inputs/link.js",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/private.js",
						},
						{
							path: "/inputs/closed/link.js",
							type: "symlink",
							mode: "0777",
							uid: 0,
							gid: 0,
							target: "/inputs/public.js",
						},
						...(
							[
								["private-chain", "private.js"],
								["public-chain", "public.js"],
							] as const
						).map(([name, interpreter]) => ({
							path: `/inputs/${name}`,
							type: "file" as const,
							mode: "0755",
							uid: 0,
							gid: 0,
							content: `#!/inputs/${interpreter}\n`,
							encoding: "utf8" as const,
						})),
					]),
				],
			},
		});
		return vm;
	}

	const syncChecks = `
		const assert = require('node:assert/strict');
		const cp = require('node:child_process');
		const opts = {uid:1000,gid:1000,cwd:'/inputs',encoding:'utf8'};
		assert.equal(cp.spawnSync('/inputs/private.wasm',[],{encoding:'utf8'}).status,23);
		for(let i=0;i<3;i++) for(const path of ${JSON.stringify(paths)}) {
			const denied = cp.spawnSync(path,[],opts);
			assert.equal(denied.error?.code,'EACCES',JSON.stringify({path,...denied}));
		}
		for(const path of ['/inputs/public.wasm','/inputs/execute-only.wasm']) {
			const allowed = cp.spawnSync(path,[],opts);
			assert.equal(allowed.status,23,JSON.stringify(allowed));
			assert.equal(allowed.error,undefined);
		}
        const unreadable=cp.spawnSync('node',['-e','require("node:fs").readFileSync("/inputs/execute-only.wasm")'],opts);
        assert.notEqual(unreadable.status,0);assert(String(unreadable.stderr).includes('EACCES'));
		assert.equal(process.getuid(),0);
		console.log('exec-authorization-ok');
	`;

	test("sync launch uses dropped child identity and preserves execute-only images", async () => {
		const current = await createVm();
		const result = await current.execArgv("node", ["-e", syncChecks]);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("exec-authorization-ok\n");
	});

	test("nested launch uses dropped child identity and reaps failed provisional children", async () => {
		const current = await createVm();
		const result = await current.execArgv("node", [
			"-e",
			`
			const assert=require('node:assert/strict');
			const child=require('node:child_process').spawnSync('node',['-e',${JSON.stringify(syncChecks)}],{encoding:'utf8'});
			assert.equal(child.status,0,JSON.stringify(child));
			process.stdout.write(child.stdout);
		`,
		]);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("exec-authorization-ok\n");
	});

	test("async launch rejects protected images before execution and leaves capacity usable", async () => {
		const current = await createVm();
		const result = await current.execArgv("node", [
			"-e",
			`
			const assert=require('node:assert/strict');
			const cp=require('node:child_process');
			(async()=>{
				for(const path of ${JSON.stringify(paths)}) {
					const error=await new Promise((resolve,reject)=>{
						const child=cp.spawn(path,[],{uid:1000,gid:1000,cwd:'/inputs'});
						child.once('error',resolve);
						child.once('exit',code=>reject(new Error('protected command executed: '+code)));
					});
					assert.equal(error.code,'EACCES',String(error));
				}
				const status=await new Promise((resolve,reject)=>{
					const child=cp.spawn('/inputs/execute-only.wasm',[],{uid:1000,gid:1000});
					child.once('error',reject);child.once('close',resolve);
				});
				assert.equal(status,23);console.log('async-authorization-ok');
			})().catch(error=>{console.error(error);process.exitCode=1;});
		`,
		]);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("async-authorization-ok\n");
	});
	for (const nested of [false, true]) {
		test(`direct JavaScript executable paths enforce execute and search permissions (${nested ? "nested" : "root"} child)`, async () => {
			const current = await createVm();
			const checks = `
                const assert=require('node:assert/strict');
                const cp=require('node:child_process');
                const opts={uid:1000,gid:1000,cwd:'/inputs',encoding:'utf8'};
                for(let i=0;i<3;i++) for(const path of ['/inputs/private.js','./private.js','/inputs/readable.js','/inputs/link.js','/inputs/closed/link.js','/inputs/private-chain']) {
                    const denied=cp.spawnSync(path,[],opts);
                    assert.equal(denied.error?.code,'EACCES',JSON.stringify({path,...denied}));
                    assert.equal(denied.stdout,'');
                }
                for(const path of ['/inputs/public.js','/inputs/public-chain']) {
                    const allowed=cp.spawnSync(path,[],opts);
                    assert.equal(allowed.status,0,JSON.stringify(allowed));
                    assert.equal(allowed.stdout,'script-executed\\n');
                }
                // Passing a readable non-executable file to Node is an input
                // read, and must not acquire a new execute-bit requirement.
                const input=cp.spawnSync('node',['/inputs/readable.js'],opts);
                assert.equal(input.status,0,JSON.stringify(input));
                assert.equal(input.stdout,'script-executed\\n');
                console.log('script-authorization-ok');
            `;
			const source = nested
				? `
                const assert=require('node:assert/strict');
                const child=require('node:child_process').spawnSync('node',['-e',${JSON.stringify(checks)}],{encoding:'utf8'});
                assert.equal(child.status,0,JSON.stringify(child));process.stdout.write(child.stdout);
            `
				: checks;
			const result = await current.execArgv("node", ["-e", source]);
			expect(result.exitCode, JSON.stringify(result)).toBe(0);
			expect(result.stdout).toBe("script-authorization-ok\n");
		});
	}
	for (const nested of [false, true]) {
		test(`Node interpreter input preserves original traversal authorization (${nested ? "nested" : "root"} child)`, async () => {
			const current = await createVm();
			const checks = `
                const assert=require('node:assert/strict');
                const cp=require('node:child_process');
                const opts={uid:1000,gid:1000,cwd:'/inputs',encoding:'utf8'};
                for(let i=0;i<3;i++) for(const path of ['/inputs/closed/link.js','./closed/link.js','/inputs/closed/../readable.js']) {
                    const denied=cp.spawnSync('node',[path],opts);
                    assert.equal(denied.error?.code,'EACCES',JSON.stringify({path,...denied}));
                    assert.equal(denied.stdout,'');
                }
                const read=cp.spawnSync('node',['-e','require("node:fs").readFileSync("/inputs/closed/link.js")'],opts);
                assert.notEqual(read.status,0);assert(String(read.stderr).includes('EACCES'));
                const allowed=cp.spawnSync('node',['/inputs/readable.js'],opts);
                assert.equal(allowed.status,0,JSON.stringify(allowed));
                assert.equal(allowed.stdout,'script-executed\\n');
                console.log('input-authorization-ok');
            `;
			const source = nested
				? `
                const assert=require('node:assert/strict');
                const child=require('node:child_process').spawnSync('node',['-e',${JSON.stringify(checks)}],{encoding:'utf8'});
                assert.equal(child.status,0,JSON.stringify(child));process.stdout.write(child.stdout);
            `
				: checks;
			const result = await current.execArgv("node", ["-e", source]);
			expect(result.exitCode, JSON.stringify(result)).toBe(0);
			expect(result.stdout).toBe("input-authorization-ok\n");
		});
	}
	test("interpreter input follows the canonical target read policy", async () => {
		const current = await createVm(true);
		const result = await current.execArgv("node", [
			"-e",
			`
            const assert=require('node:assert/strict');
            const cp=require('node:child_process'), fs=require('node:fs');
            assert.throws(()=>fs.readFileSync('/inputs/input-link.js'),{code:'EACCES'});
            assert(fs.readFileSync('/inputs/allowed-input-link.js','utf8').includes('script-executed'));
            const opts={uid:1000,gid:1000,encoding:'utf8'};
            for(let i=0;i<6;i++) {
                const denied=cp.spawnSync('node',['/inputs/input-link.js'],opts);
                assert.equal(denied.error?.code,'EACCES',JSON.stringify(denied));
                assert.equal(denied.stdout,'');
            }
            const allowed=cp.spawnSync('node',['/inputs/allowed-input-link.js'],opts);
            assert.equal(allowed.status,0,JSON.stringify(allowed));
            assert.equal(allowed.stdout,'script-executed\\n');
            console.log('input-policy-ok');
        `,
		]);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("input-policy-ok\n");
	});
});
