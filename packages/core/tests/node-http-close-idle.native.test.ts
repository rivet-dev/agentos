import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function compare(source: string) {
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 7000 },
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
		const p = await vm.process.spawn(
			"node",
			["--input-type=module", "-e", source],
			{
				timeoutMs: 7000,
				onStdout: (b) => (stdout += Buffer.from(b).toString()),
				onStderr: (b) => (stderr += Buffer.from(b).toString()),
			},
		);
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}
const setup = `import http from'node:http';import net from'node:net';import{setTimeout as delay}from'node:timers/promises';const connections=new Set();let finish;const server=http.createServer((req,res)=>{if(req.url==='/held'){res.writeHead(200,{'content-length':'9'});res.write('first');finish=()=>res.end('last');}else res.end('ok');});server.on('connection',s=>{connections.add(s);s.once('close',()=>connections.delete(s));});await new Promise(r=>server.listen(0,'127.0.0.1',r));const socket=net.connect(server.address().port,'127.0.0.1');socket.on('error',()=>{});await new Promise(r=>socket.once('connect',r));let body='';socket.on('data',b=>body+=b);const until=async predicate=>{const deadline=Date.now()+2000;while(!predicate()){if(Date.now()>deadline)throw Error('response missing');await delay(5);}};`;

test("HTTP Server.close reaps an idle keep-alive connection without client disposal", async () => {
	await compare(
		setup +
			`let closed=false;try{socket.write('GET /idle HTTP/1.1\\r\\nHost: test\\r\\nConnection: keep-alive\\r\\n\\r\\n');await until(()=>body.endsWith('ok'));await delay(10);server.close(()=>{closed=true;});await Promise.race([until(()=>closed),delay(500)]);console.log(JSON.stringify({closedWithoutClientDestroy:closed,response:body.endsWith('ok')}));}finally{socket.destroy();for(const s of connections)s.destroy();server.close();}`,
	);
}, 12000);

test("HTTP Server.close preserves an active response and its connection", async () => {
	await compare(
		setup +
			`let closed=false;try{socket.write('GET /held HTTP/1.1\\r\\nHost: test\\r\\nConnection: keep-alive\\r\\n\\r\\n');await until(()=>body.endsWith('first'));server.close(()=>{closed=true;});await delay(30);const keptActive=!closed&&!socket.destroyed;finish();await until(()=>body.endsWith('firstlast'));await Promise.race([until(()=>closed),delay(500)]);console.log(JSON.stringify({keptActive,closedAfterResponse:closed,completeBody:body.endsWith('firstlast')}));}finally{socket.destroy();for(const s of connections)s.destroy();server.close();}`,
	);
}, 12000);

test("HTTP Server.close refuses new connections while an active response drains", async () => {
	await compare(
		setup +
			`let refused=false;try{const port=server.address().port;socket.write('GET /held HTTP/1.1\\r\\nHost: test\\r\\nConnection: keep-alive\\r\\n\\r\\n');await until(()=>body.endsWith('first'));server.close();const probe=net.connect(port,'127.0.0.1');await new Promise(resolve=>{probe.once('error',e=>{refused=e.code==='ECONNREFUSED';resolve();});probe.once('connect',()=>{probe.destroy();resolve();});});finish();await until(()=>body.endsWith('firstlast'));console.log(JSON.stringify({refused,completeBody:body.endsWith('firstlast')}));}finally{socket.destroy();for(const s of connections)s.destroy();server.close();}`,
	);
}, 12000);
