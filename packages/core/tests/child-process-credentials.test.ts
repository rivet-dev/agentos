import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

describe("child process credentials", () => {
	let vm: AgentOs | undefined;
	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	async function run(uid: number, code: string) {
		vm = await AgentOs.create({
			defaultSoftware: uid === 0,
			user: { uid, gid: uid, supplementaryGids: [42] },
			permissions: { fs: "allow", childProcess: "allow", process: "allow" },
			limits: { resources: { maxProcesses: 4 } },
		});
		return vm.execArgv("node", ["-e", code]);
	}

	test("root drops child credentials through sync, async, and shell launch without changing itself", async () => {
		const result = await run(
			0,
			`
			const assert = require('node:assert/strict');
			const cp = require('node:child_process');
			const read = 'console.log(JSON.stringify({uid:process.getuid(),gid:process.getgid(),groups:process.getgroups()}))';
			const opts = {uid:1001,gid:1002,encoding:'utf8'};
			const before = [process.getuid(),process.getgid(),process.getgroups()];
			const check = text => {
				const id = JSON.parse(text);
				assert.equal(id.uid,1001); assert.equal(id.gid,1002);
				assert(!id.groups.includes(0)); assert(!id.groups.includes(42));
			};
			const sync = cp.spawnSync('node',['-e',read],opts);
			assert.equal(sync.status,0,JSON.stringify(sync)); check(sync.stdout);
			check(cp.execFileSync('node',['-e',read],opts));
			check(cp.execSync("node -e '"+read+"'",opts));
			const child = cp.spawn('node',['-e',read],opts);
			let output=''; child.stdout.on('data', chunk => output+=chunk);
			child.on('error', error => {throw error;});
			child.on('close', status => {
				assert.equal(status,0); check(output);
				assert.deepEqual([process.getuid(),process.getgid(),process.getgroups()],before);
				console.log('credentials-ok');
			});
		`,
		);
		expect(result.stderr).toBe("");
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("credentials-ok\n");
	});

	test("unprivileged children cannot change to root and rejected launches release process slots", async () => {
		const result = await run(
			1000,
			`
			const assert = require('node:assert/strict');
			const cp = require('node:child_process');
			for (let i=0;i<6;i++) {
				for (const options of [{uid:0},{gid:0}]) {
					const denied = cp.spawnSync('node',['-e',"console.log('must-not-run')"],options);
					assert.equal(denied.error?.code,'EPERM',JSON.stringify(denied));
					assert(!String(denied.stdout).includes('must-not-run'));
				}
			}
			const allowed = cp.spawnSync('node',['-e','console.log(process.getuid()+":"+process.getgid())'],{uid:1000,gid:1000,encoding:'utf8'});
			assert.equal(allowed.status,0,JSON.stringify(allowed));
			assert.equal(allowed.stdout,'1000:1000\\n');
			const child = cp.spawn('node',['-e',"console.log('must-not-run')"],{uid:0});
			let output=''; child.stdout.on('data', chunk=>output+=chunk);
			child.on('error', error => {
				assert.equal(error.code,'EPERM');
				assert.equal(output,'');
				assert.equal(process.getuid(),1000);
				console.log('denial-ok');
			});
		`,
		);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("denial-ok\n");
	});

	test("invalid credential arguments fail before launch and null preserves inheritance", async () => {
		const result = await run(
			1000,
			`
			const assert = require('node:assert/strict');
			const cp = require('node:child_process');
			for (const name of ['uid','gid']) {
				for (const value of ['1000',1.5,NaN,Infinity,2147483648,-2147483649]) {
					for (const launch of [
						options=>cp.spawn('node',['-e',''],options),
						options=>cp.spawnSync('node',['-e',''],options),
						options=>cp.execSync('node -e ""',options),
					]) assert.throws(()=>launch({[name]:value}),{code:'ERR_INVALID_ARG_TYPE'});
				}
				const invalid = cp.spawnSync('node',['-e',''],{[name]:-1});
				assert.equal(invalid.error?.code,'EINVAL',JSON.stringify(invalid));
			}
			const child = cp.spawnSync('node',['-e','console.log(process.getuid())'],{uid:null,gid:null,encoding:'utf8'});
			assert.equal(child.status,0); assert.equal(child.stdout,'1000\\n');
			console.log('validation-ok');
		`,
		);
		expect(result.exitCode, JSON.stringify(result)).toBe(0);
		expect(result.stdout).toBe("validation-ok\n");
	});
});
