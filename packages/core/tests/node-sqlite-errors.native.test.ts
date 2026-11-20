import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("node:sqlite retains SQLite messages and extended result codes", async () => {
	const source = `import{DatabaseSync}from'node:sqlite';const d=new DatabaseSync(':memory:');d.exec('CREATE TABLE t(a TEXT UNIQUE,b INTEGER NOT NULL CHECK(b>0));');const errors=[];
for(const [kind,sql]of [['exec','INSERT INTO missing VALUES(1)'],['prepare','SELECT bad + FROM t'],['run',"INSERT INTO t VALUES('x',1)"],['exec',"INSERT INTO t VALUES('n',NULL)"],['run',"INSERT INTO t VALUES('z',0)"]]){try{if(kind==='exec')d.exec(sql);else if(kind==='prepare')d.prepare(sql);else{d.prepare(sql).run();d.prepare(sql).run();}throw Error('expected failure');}catch(e){errors.push({name:e.name,message:e.message,code:e.code,errcode:e.errcode,errstr:e.errstr});}}d.close();console.log(JSON.stringify(errors));`;
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 5000, maxBuffer: 16384 },
	);
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		let stdout = "",
			stderr = "";
		const proc = await vm.process.spawn(
			"node",
			["--input-type=module", "-e", source],
			{
				timeoutMs: 5000,
				onStdout: (b) => {
					stdout += Buffer.from(b).toString();
				},
				onStderr: (b) => {
					stderr += Buffer.from(b).toString();
				},
			},
		);
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 10000);
