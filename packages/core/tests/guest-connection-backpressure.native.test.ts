import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("a paused published stream applies backpressure without consuming the VM socket budget", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const ready = Promise.withResolvers<void>();
		const paused = Promise.withResolvers<void>();
		let stdout = "",
			stderr = "";
		const proc = await vm.process.spawn(
			"node",
			[
				"-e",
				`
const net=require('node:net'); const sockets=[]; let resumed=false;
const server=net.createServer(socket=>{sockets.push(socket);socket.pause();console.log('paused');let size=0;socket.on('data',b=>size+=b.length);socket.on('end',()=>{socket.end(String(size));server.close();});if(resumed)socket.resume();});
process.stdin.once('data',()=>{resumed=true;for(const socket of sockets)socket.resume();process.stdin.pause();});
server.listen(18788,'127.0.0.1',()=>console.log('ready'));`,
			],
			{
				streamStdin: true,
				timeoutMs: 10000,
				onStdout: (b) => {
					stdout += Buffer.from(b).toString();
					if (stdout.includes("ready")) ready.resolve();
					if (stdout.includes("paused")) paused.resolve();
				},
				onStderr: (b) => {
					stderr += Buffer.from(b).toString();
				},
			},
		);
		await ready.promise;
		const connection = await vm.network.connect(18788);
		try {
			await paused.promise;
			let settled = false;
			const payload = Buffer.alloc(1024 * 1024, 65);
			const writing = connection.write(payload).then(() => {
				settled = true;
			});
			await new Promise((resolve) => setTimeout(resolve, 150));
			const completedBeforeRead = settled;
			await vm.process.writeStdin(proc.pid, "resume\n");
			await writing;
			await connection.end();
			const chunks: Uint8Array[] = [];
			for (;;) {
				const chunk = await connection.read();
				chunks.push(chunk.body);
				if (chunk.done) break;
			}
			expect(Buffer.concat(chunks).toString()).toBe(String(payload.length));
			expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
			expect(completedBeforeRead).toBe(false);
		} finally {
			await connection.close();
		}
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 15000);

test("large guest writes retain every byte across a published receive window", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const ready = Promise.withResolvers<void>();
		let stderr = "";
		const proc = await vm.process.spawn(
			"node",
			[
				"-e",
				`const net=require('node:net');const server=net.createServer(socket=>socket.end(Buffer.alloc(1024*1024,66),()=>server.close()));server.listen(18789,'127.0.0.1',()=>console.log('ready'));`,
			],
			{
				timeoutMs: 10000,
				onStdout: (b) => {
					if (Buffer.from(b).toString().includes("ready")) ready.resolve();
				},
				onStderr: (b) => {
					stderr += Buffer.from(b).toString();
				},
			},
		);
		await ready.promise;
		const connection = await vm.network.connect(18789);
		try {
			const chunks: Uint8Array[] = [];
			for (;;) {
				const chunk = await connection.read();
				chunks.push(chunk.body);
				if (chunk.done) break;
			}
			const received = Buffer.concat(chunks);
			expect(received.length).toBe(1024 * 1024);
			expect(received.every((byte) => byte === 66)).toBe(true);
			await connection.end();
			expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		} finally {
			await connection.close();
		}
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 15000);
