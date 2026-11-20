import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("a transferred HTTP socket does not retain its completed ClientRequest", async () => {
	const peers = new Set<import("node:net").Socket>();
	const server = createServer();
	server.on("connection", (socket) => {
		peers.add(socket);
		socket.on("error", () => {});
		socket.once("close", () => peers.delete(socket));
	});
	server.on("upgrade", (_request, socket) => {
		socket.write(
			"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: fixture\r\n\r\n",
		);
		socket.on("data", (bytes) => socket.write(bytes));
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string")
		throw Error("No fixture address");
	const source = [
		"import http from'node:http';import{setImmediate as tick}from'node:timers/promises';",
		"async function open(){let request=http.request({host:'127.0.0.1',port:PORT,agent:false,headers:{Connection:'Upgrade',Upgrade:'fixture'}});let socket;await new Promise((resolve,reject)=>{request.once('error',reject);request.once('upgrade',(_res,s)=>{socket=s;resolve();});request.end();});const reference=new WeakRef(request);request=null;return{reference,socket};}",
		"const{reference,socket}=await open();try{for(let i=0;i<4;i++){gc();await tick();}const collected=reference.deref()===undefined;const response=await new Promise(resolve=>{socket.once('data',b=>resolve(b.toString()));socket.write('ping');});console.log(JSON.stringify({collected,response,socketAlive:!socket.destroyed}));}finally{socket.destroy();}",
	]
		.join("\n")
		.replace("PORT", String(address.port));
	let vm: AgentOs | undefined;
	const sidecar = await AgentOs.createSidecar();
	try {
		const host = await new Promise<{
			code: number | null;
			stdout: string;
			stderr: string;
		}>((resolve, reject) => {
			const child = spawn(
				process.execPath,
				["--expose-gc", "--input-type=module", "-e", source],
				{ stdio: ["ignore", "pipe", "pipe"] },
			);
			let stdout = "",
				stderr = "";
			child.stdout.on("data", (bytes) => {
				stdout += bytes;
			});
			child.stderr.on("data", (bytes) => {
				stderr += bytes;
			});
			const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
			child.once("error", (error) => {
				clearTimeout(timer);
				reject(error);
			});
			child.once("exit", (code) => {
				clearTimeout(timer);
				resolve({ code, stdout, stderr });
			});
		});
		expect(host.code, host.stderr).toBe(0);
		const expected = { collected: true, response: "ping", socketAlive: true };
		expect(JSON.parse(host.stdout)).toEqual(expected);
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: {
				network: {
					default: "deny",
					rules: [
						{ mode: "allow", patterns: ["tcp://127.0.0.1:" + address.port] },
					],
				},
				hostFunction: "deny",
			},
			loopbackExemptPorts: [address.port],
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn(
			"node",
			["--expose-gc", "--input-type=module", "-e", source],
			{
				timeoutMs: 5000,
				onStdout: (bytes) => {
					stdout += Buffer.from(bytes).toString();
				},
				onStderr: (bytes) => {
					stderr += Buffer.from(bytes).toString();
				},
			},
		);
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(expected);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
		for (const socket of peers) socket.destroy();
		await new Promise<void>((resolve) => server.close(() => resolve()));
	}
}, 15000);
