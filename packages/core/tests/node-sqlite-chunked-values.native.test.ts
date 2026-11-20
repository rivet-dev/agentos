import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function compare(source: string) {
	const host = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 15000,
	});
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { jsRuntime: { v8HeapLimitMb: 512 } },
		});
		let stdout = "",
			stderr = "";
		const p = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 15000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}
test("SQLite get/all transfer large BLOB results through bounded bridge responses", async () => {
	await compare(
		`const{DatabaseSync}=require('node:sqlite'),{createHash}=require('node:crypto');const db=new DatabaseSync(':memory:');db.exec('CREATE TABLE t(v BLOB); INSERT INTO t VALUES(zeroblob(20971520));');const s=db.prepare('SELECT v FROM t');const digest=v=>[v.byteLength,createHash('sha256').update(v).digest('hex'),v instanceof Uint8Array];console.log(JSON.stringify({get:digest(s.get().v),all:s.all().map(r=>digest(r.v))}));db.close();`,
	);
}, 20000);
test("SQLite large BLOB parameters preserve exact bytes without oversized requests", async () => {
	await compare(
		`const{DatabaseSync}=require('node:sqlite'),{createHash}=require('node:crypto');const db=new DatabaseSync(':memory:');db.exec('CREATE TABLE t(v BLOB)');const b=Buffer.alloc(20971520);for(let i=0;i<b.length;i++)b[i]=i%251;db.prepare('INSERT INTO t VALUES(?)').run(b);const row=db.prepare('SELECT v FROM t').get();console.log(JSON.stringify({length:row.v.byteLength,digest:createHash('sha256').update(row.v).digest('hex')}));db.close();`,
	);
}, 20000);
