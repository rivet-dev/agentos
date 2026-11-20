import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("SQLite commits are visible to filesystem reads before database close", async () => {
	const source = `import {DatabaseSync} from 'node:sqlite';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sqlite-visible-'));
try{
const file=path.join(dir,'db.sqlite');const db=new DatabaseSync(file);
db.exec("PRAGMA journal_mode=DELETE;CREATE TABLE sample(id INTEGER PRIMARY KEY,value TEXT);BEGIN;INSERT INTO sample VALUES(7,'first');COMMIT;");
const observations=[];
const snapshot=()=>{const copy=path.join(dir,'copy.sqlite');fs.writeFileSync(copy,fs.readFileSync(file));const handle=new DatabaseSync(copy);observations.push(handle.prepare('SELECT * FROM sample ORDER BY id').all());handle.close();};
snapshot();db.exec('/* transaction */ BEGIN; SAVEPOINT nested;');db.prepare('INSERT INTO sample VALUES(?,?)').run(8,'second');
db.exec('RELEASE nested;COMMIT;');snapshot();
db.prepare('UPDATE sample SET value=? WHERE id=7 RETURNING id').all('updated');snapshot();
db.exec('BEGIN;DELETE FROM sample;ROLLBACK;');snapshot();
const parsed=db.prepare(" SELECT 'quoted;λ' AS value; SELECT 2;");
const state=[db.isTransaction];db.exec('BEGIN');state.push(db.isTransaction);db.exec('ROLLBACK');state.push(db.isTransaction);
observations.push({sourceSQL:parsed.sourceSQL,value:parsed.get(),state});db.close();
console.log(JSON.stringify(observations));
}finally{fs.rmSync(dir,{recursive:true,force:true});}`;
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
				onStdout: (c) => {
					stdout += Buffer.from(c).toString();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
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
