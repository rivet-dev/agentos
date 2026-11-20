import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("crypto Hash copies fork immutable input and retain independent finalization", async () => {
	const source = `import {createHash} from 'node:crypto';
const results={};
for(const algorithm of ['md5','sha1','sha224','sha256','sha384','sha512']){
 const bytes=Buffer.from([9,1,2,9]);const root=createHash(algorithm).update(bytes.subarray(1,3));
 const left=root.copy(),right=root.copy();bytes.fill(0);
 const branch=left.update('left').copy().update('branch');
 results[algorithm]=[root.update('root').digest('hex'),left.digest('hex'),right.update('right').digest('hex'),branch.digest('hex')];
}
results.empty=createHash('sha256').copy().digest('hex');
results.length=createHash('sha256').update('s3').copy({outputLength:32}).digest('hex');
results.invalid=[];for(const outputLength of [1,'32',NaN,-1]){
 try{createHash('sha256').copy({outputLength});results.invalid.push('accepted');}catch(e){results.invalid.push(e.code);}
}
const finalized=createHash('sha256');finalized.digest();results.finalized=[];
for(const operation of ['copy','update','digest']){try{finalized[operation]('x');results.finalized.push('accepted');}catch(e){results.finalized.push([e.name,e.code,e.message]);}}
results.sizes=[];
for(const size of [0,65536,1048576,1048577,20*1048576]){
 const bytes=Buffer.alloc(size,37),hash=createHash('sha256').update(bytes);
 const copy=hash.copy();bytes.fill(0);
 results.sizes.push([size,hash.digest('hex'),copy.digest('hex')]);
}
console.log(JSON.stringify(results));`;
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
		const child = await vm.process.spawn(
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
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 10000);
