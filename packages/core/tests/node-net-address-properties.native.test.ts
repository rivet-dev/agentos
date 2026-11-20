import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const source = `import net from 'node:net';import{once}from'node:events';import{rm}from'node:fs/promises';
const names=['localAddress','localPort','localFamily','remoteAddress','remotePort','remoteFamily'];
const own=socket=>names.map(name=>Object.hasOwn(socket,name));const blank=new net.Socket();
const initial={own:own(blank),unset:names.every(name=>blank[name]===undefined)};blank.destroy();
async function connected(path){let accepted;const server=net.createServer(socket=>{accepted=socket;});
await new Promise((resolve,reject)=>{server.once('error',reject);path?server.listen(path,resolve):server.listen(0,'127.0.0.1',resolve);});
const client=path?net.connect(path):net.connect(server.address().port,'127.0.0.1');
try{await once(client,'connect');const result={clientOwn:own(client),acceptedOwn:own(accepted),addresses:path?names.every(name=>client[name]===undefined&&accepted[name]===undefined):client.localAddress==='127.0.0.1'&&client.remoteAddress==='127.0.0.1'&&client.localFamily==='IPv4'&&client.remoteFamily==='IPv4'&&client.localPort===accepted.remotePort&&client.remotePort===accepted.localPort};return result;}
finally{client.destroy();accepted?.destroy();await new Promise(resolve=>server.close(resolve));if(path)await rm(path,{force:true});}}
console.log(JSON.stringify({initial,tcp:await connected(),unix:await connected('/tmp/net-address-properties-'+process.pid+'.sock')}));`;
const absent = Array(6).fill(false);
const expected = {
	initial: { own: absent, unset: true },
	tcp: { clientOwn: absent, acceptedOwn: absent, addresses: true },
	unix: { clientOwn: absent, acceptedOwn: absent, addresses: true },
};

test("TCP and Unix socket address accessors match Node without own address properties", async () => {
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
}, 15000);
