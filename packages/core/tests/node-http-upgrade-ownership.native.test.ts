import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const setup = `import http from 'node:http';import{setImmediate as tick,setTimeout as delay}from'node:timers/promises';
const peers=new Set();const server=http.createServer((_req,res)=>res.end('ok'));
server.on('connection',socket=>{peers.add(socket);socket.once('close',()=>peers.delete(socket));});
server.on('upgrade',(_req,socket)=>{socket.write('HTTP/1.1 101 Switching Protocols\\r\\nConnection: Upgrade\\r\\nUpgrade: fixture\\r\\n\\r\\n');socket.on('data',bytes=>socket.write(bytes));});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const agent=new http.Agent({keepAlive:true,maxSockets:1});let upgraded,requestClosed=false;
const options={host:'127.0.0.1',port:server.address().port,agent,headers:{Connection:'Upgrade',Upgrade:'fixture'}};
const request=http.request(options);request.on('close',()=>{requestClosed=true;});
try{await new Promise((resolve,reject)=>{request.once('error',reject);request.once('upgrade',(_res,socket)=>{upgraded=socket;resolve();});request.end();});await tick();`;
const cleanup = `}finally{upgraded?.destroy();agent.destroy();for(const socket of peers)socket.destroy();await new Promise(resolve=>server.close(resolve));}`;

async function compare(source: string, expected: unknown): Promise<void> {
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 6000 },
	);
	expect(host.status, host.stderr).toBe(0);
	expect(JSON.parse(host.stdout)).toEqual(expected);
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
				timeoutMs: 6000,
				onStdout: (b) => {
					stdout += Buffer.from(b).toString();
				},
				onStderr: (b) => {
					stderr += Buffer.from(b).toString();
				},
			},
		);
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(expected);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}

test("successful HTTP upgrade finishes the request while its transferred socket still exchanges bytes", async () => {
	await compare(
		setup +
			`const echoed=new Promise(resolve=>upgraded.once('data',bytes=>resolve(bytes.toString())));upgraded.write('ping');const message=await echoed;console.log(JSON.stringify({requestClosed,requestDestroyed:request.destroyed,socketAlive:!upgraded.destroyed,message}));` +
			cleanup,
		{
			requestClosed: true,
			requestDestroyed: true,
			socketAlive: true,
			message: "ping",
		},
	);
}, 15000);

test("successful HTTP upgrade releases a one-socket Agent slot for another request", async () => {
	await compare(
		setup +
			`const activeAfterUpgrade=Object.values(agent.sockets).flat().length;const response=await new Promise(resolve=>{const next=http.get({host:options.host,port:options.port,agent},res=>{let text='';res.on('data',bytes=>text+=bytes);res.on('end',()=>resolve(text));});next.once('error',()=>resolve('failed'));const deadline=setTimeout(()=>{next.destroy();resolve('timed out');},1000);next.once('close',()=>clearTimeout(deadline));});console.log(JSON.stringify({activeAfterUpgrade,response,socketAlive:!upgraded.destroyed}));` +
			cleanup,
		{ activeAfterUpgrade: 0, response: "ok", socketAlive: true },
	);
}, 15000);
