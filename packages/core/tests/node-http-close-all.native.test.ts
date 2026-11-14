import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("HTTP closeAllConnections closes active and idle HTTP sockets while preserving upgrades", async () => {
	const source = `import http from 'node:http';import net from 'node:net';import{once}from'node:events';
const busy=Promise.withResolvers();const server=http.createServer(()=>busy.resolve());let upgradedPeer;
server.on('upgrade',(request,socket)=>{upgradedPeer=socket;socket.write('HTTP/1.1 101 Switching Protocols\\r\\nConnection: Upgrade\\r\\nUpgrade: echo\\r\\n\\r\\n');socket.on('data',bytes=>socket.write(bytes));});
server.listen(0,'127.0.0.1');await once(server,'listening');const port=server.address().port;
const connect=async()=>{const socket=net.connect(port,'127.0.0.1');await once(socket,'connect');return socket;};
const idle=await connect();const active=await connect();active.write('GET /busy HTTP/1.1\\r\\nHost: vm\\r\\n\\r\\n');await busy.promise;
const upgraded=await connect();const headers=once(upgraded,'data');upgraded.write('GET /upgrade HTTP/1.1\\r\\nHost: vm\\r\\nConnection: Upgrade\\r\\nUpgrade: echo\\r\\n\\r\\n');await headers;
const closed=Promise.all([once(idle,'close'),once(active,'close')]);server.closeAllConnections();server.closeAllConnections();await closed;
const echoed=once(upgraded,'data');upgraded.write('still-open');const [bytes]=await echoed;
const ended=once(upgraded,'close');upgraded.destroy();await ended;upgradedPeer.destroy();const stopped=once(server,'close');server.close();await stopped;
console.log(JSON.stringify({active:active.destroyed,idle:idle.destroyed,upgrade:bytes.toString()}));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 10000, maxBuffer: 16384 });
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar(); let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: { network: "deny" } });
		let stdout = "", stderr = "";
		const child = await vm.process.spawn("node", ["--input-type=module", "-e", source], { timeoutMs: 10000,
			onStdout: bytes => { stdout += Buffer.from(bytes).toString(); }, onStderr: bytes => { stderr += Buffer.from(bytes).toString(); },
		});
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 20000);
