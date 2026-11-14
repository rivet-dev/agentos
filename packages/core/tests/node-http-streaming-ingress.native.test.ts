import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("HTTP servers consume chunked and fixed-length input before upload EOF", async () => {
	const source = `import http from 'node:http';import net from 'node:net';
const server=http.createServer(async(request,response)=>{
 try{const iterator=request.iterator({destroyOnReturn:false});const first=await iterator.next();await iterator.return();response.end(first.value);}
 catch(error){response.statusCode=500;response.end(error.message);}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const results=[];
try{for(const chunked of [true,false]){
 const socket=net.createConnection({host:'127.0.0.1',port:server.address().port});let data='';
 try{results.push(await new Promise((resolve,reject)=>{
 const timeout=setTimeout(()=>reject(new Error('response waited for upload EOF')),1500);
 socket.on('error',reject);socket.on('data',chunk=>{data+=chunk.toString();if(data.includes('first')){clearTimeout(timeout);resolve('first');}});
 socket.on('connect',()=>{socket.write('POST / HTTP/1.1\\r\\nHost: localhost\\r\\nConnection: close\\r\\n'+(chunked?'Transfer-Encoding: chunked':'Content-Length: 10')+'\\r\\n\\r\\n');socket.write(chunked?'5;extension=value\\r\\nfirst\\r\\n':'first');});
 }));}finally{socket.destroy();}
}}finally{await new Promise(r=>server.close(r));}console.log(JSON.stringify(results));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 6000, maxBuffer: 16384 });
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar(); let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: { network: "deny" } });
		let stdout = "", stderr = "";
		const proc = await vm.process.spawn("node", ["--input-type=module", "-e", source], { timeoutMs: 6000,
			onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); },
		});
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 10000);
