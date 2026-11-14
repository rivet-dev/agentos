import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("HTTP server requests provide canonical async iterators without destroying on return", async () => {
	const source = `import http from 'node:http';
const server=http.createServer(async(request,response)=>{try{
 let invalid;try{request.iterator(null);}catch(error){invalid=error.code;}
 const iterator=request.iterator({destroyOnReturn:false});const first=await iterator.next();
 await iterator.return();const preserved=!request.destroyed;
 const rest=[];for await(const chunk of request)rest.push(chunk);
 response.end(JSON.stringify({first:first.value?.toString()??null,done:first.done,preserved,rest:Buffer.concat(rest).toString(),invalid}));
}catch(error){response.end(JSON.stringify({error:error.message}));}});
await new Promise((r,j)=>{server.once('error',j);server.listen(0,'127.0.0.1',r);});
const rows=[];for(const body of ['payload',''])rows.push(await new Promise((r,j)=>{
 const request=http.request({hostname:'127.0.0.1',port:server.address().port,method:'POST',headers:{'content-length':Buffer.byteLength(body)}},response=>{let data='';response.on('data',c=>data+=c);response.on('end',()=>r(JSON.parse(data)));});request.once('error',j);request.end(body);
}));await new Promise(r=>server.close(r));console.log(JSON.stringify(rows));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 5000, maxBuffer: 16384 });
	expect(host.status, host.stderr).toBe(0);
	const expected = JSON.parse(host.stdout);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: { network: "deny" } });
		let stdout = "", stderr = "";
		const proc = await vm.process.spawn("node", ["--input-type=module", "-e", source], {
			timeoutMs: 5000, onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); },
		});
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(expected);
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 10000);
