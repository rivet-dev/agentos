import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("SQLite result quota failures publish committed RETURNING mutations before close", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { sqlite: { maxResultBytes: 1024 * 1024 } },
		});
		const source = `const {DatabaseSync}=require('node:sqlite'),fs=require('node:fs');const observations=[];
  for(const method of ['get','all']){
   const file='/tmp/quota-'+method+'.sqlite',copy=file+'.copy',db=new DatabaseSync(file);db.exec('CREATE TABLE data(value BLOB)');
   let failure;try{db.prepare('INSERT INTO data VALUES(zeroblob(2097152)) RETURNING value')[method]();}catch(error){failure={code:error.code,message:error.message};}
   fs.writeFileSync(copy,fs.readFileSync(file));const snapshot=new DatabaseSync(copy);
   observations.push({method,failure,live:db.prepare('SELECT count(*) AS count FROM data').get().count,snapshot:snapshot.prepare('SELECT count(*) AS count FROM data').get().count});snapshot.close();db.close();
  }console.log(JSON.stringify(observations));`;
		let stdout = "",
			stderr = "";
		const process = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 10000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(process.pid)).exitCode, stderr).toBe(0);
		const rows = JSON.parse(stdout);
		expect(rows).toHaveLength(2);
		for (const row of rows) {
			expect(row.failure?.code).toBe("ERR_AGENTOS_RESOURCE_LIMIT");
			expect(row.failure.message).toContain("maxResultBytes");
			expect(row.live).toBe(1);
			expect(row.snapshot).toBe(1);
		}
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 15000);
