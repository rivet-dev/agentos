import { createServer } from "node:net";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("trusted guest connections are full duplex, bounded and confined to VM listeners", async () => {
	let hostConnections = 0;
	const host = createServer((socket) => {
		hostConnections++;
		socket.end();
	});
	await new Promise<void>((resolve) => host.listen(0, "127.0.0.1", resolve));
	const hostAddress = host.address();
	if (!hostAddress || typeof hostAddress === "string")
		throw new Error("Missing host listener");
	const previousTimeout = process.env.AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS;
	process.env.AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS = "40";
	const restoreTimeout = () => {
		if (previousTimeout === undefined)
			delete process.env.AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS;
		else process.env.AGENTOS_HTTP_LOOPBACK_REQUEST_TIMEOUT_MS = previousTimeout;
	};
	let sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>> | undefined;
	let vm: AgentOs | undefined;
	try {
		sidecar = await AgentOs.createSidecar();
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		restoreTimeout();
		await expect(vm.network.connect(hostAddress.port)).rejects.toThrow(
			/ECONNREFUSED|no VM-local listener/,
		);
		const ready = Promise.withResolvers<void>();
		let stderr = "";
		const proc = await vm.process.spawn(
			"node",
			[
				"--input-type=module",
				"-e",
				`
import net from 'node:net';
const server=net.createServer({allowHalfOpen:true},socket=>{
 socket.on('error',console.error);
 let first=true;
 socket.on('data',chunk=>{
  const echo=()=>{if(!socket.write(chunk))socket.pause();};
  if(first){first=false;setTimeout(echo,120);}else echo();
 });
 socket.on('drain',()=>socket.resume());
 socket.on('end',()=>{socket.end('end');server.close();});
});server.listen(18787,'127.0.0.1',()=>console.log('ready'));`,
			],
			{
				timeoutMs: 10000,
				onStdout: (c) => {
					if (Buffer.from(c).toString().includes("ready")) ready.resolve();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
				},
			},
		);
		await ready.promise;
		const connection = await vm.network.connect(18787);
		try {
			await connection.write(Buffer.from("first"));
			expect(Buffer.from((await connection.read()).body).toString()).toBe(
				"first",
			);
			const payload = Buffer.alloc(192 * 1024, 37);
			const received = (async () => {
				const chunks: Uint8Array[] = [];
				for (;;) {
					const chunk = await connection.read();
					chunks.push(chunk.body);
					if (chunk.done) return Buffer.concat(chunks);
				}
			})();
			await connection.write(payload);
			await connection.end();
			expect(await received).toEqual(
				Buffer.concat([payload, Buffer.from("end")]),
			);
		} finally {
			await connection.close();
		}
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		expect(hostConnections).toBe(0);
	} finally {
		restoreTimeout();
		try {
			await vm?.dispose();
		} finally {
			await sidecar?.dispose();
			await new Promise<void>((resolve, reject) =>
				host.close((error) => (error ? reject(error) : resolve())),
			);
		}
	}
}, 15000);

test("closing a parked guest read releases it without disturbing a second connection", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	const connections: Awaited<ReturnType<AgentOs["network"]["connect"]>>[] = [];
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const ready = Promise.withResolvers<void>(),
			accepted = Promise.withResolvers<void>();
		let stdout = "",
			stderr = "";
		const proc = await vm.process.spawn(
			"node",
			[
				"-e",
				`const sockets=[];require('node:net').createServer(socket=>{sockets.push(socket);socket.on('error',()=>{});if(sockets.length===2)console.log('accepted');}).listen(18790,'127.0.0.1',()=>console.log('ready'));process.stdin.on('data',()=>sockets[1].end('survivor'));`,
			],
			{
				streamStdin: true,
				timeoutMs: 10000,
				onStdout: (b) => {
					stdout += Buffer.from(b).toString();
					if (stdout.includes("ready")) ready.resolve();
					if (stdout.includes("accepted")) accepted.resolve();
				},
				onStderr: (b) => (stderr += Buffer.from(b).toString()),
			},
		);
		await ready.promise;
		connections.push(
			await vm.network.connect(18790),
			await vm.network.connect(18790),
		);
		await accepted.promise;
		const first = connections[0]!.read().then(
			() => ({ closed: false }),
			(error) => ({ closed: true, error: String(error) }),
		);
		const second = connections[1]!.read();
		await vm.filesystem.exists("/tmp/parked-read-barrier");
		await connections[0]!.close();
		expect(await first, stderr).toMatchObject({
			closed: true,
			error: expect.stringMatching(/closed|cancel/i),
		});
		await vm.process.writeStdin(proc.pid, "release\n");
		expect(Buffer.from((await second).body).toString(), stderr).toBe(
			"survivor",
		);
		for (const connection of connections) await connection.close();
		connections.length = 0;
		await vm.process.kill(proc.pid);
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 15000);
