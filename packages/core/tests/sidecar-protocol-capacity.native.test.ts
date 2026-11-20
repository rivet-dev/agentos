import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

for (const maximum of [2, 4])
	test(`protocol capacity ${maximum} remains enforced for real parked guest reads`, async () => {
		const sidecar = await AgentOs.createSidecar({
			runtime: {
				protocol: {
					maxInFlightRequests: maximum,
					maxTerminalFrames: maximum,
					maxTerminalBytes: 8 * 1024 * 1024,
				},
			},
		});
		let vm: AgentOs | undefined;
		const connections: Awaited<ReturnType<AgentOs["network"]["connect"]>>[] =
			[];
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
					`require('node:net').createServer(socket=>setTimeout(()=>socket.end('done'),1500)).listen(17171,'127.0.0.1',()=>console.log('ready'));`,
				],
				{
					timeoutMs: 15000,
					onStdout: (b) => {
						if (Buffer.from(b).toString().includes("ready")) ready.resolve();
					},
					onStderr: (b) => (stderr += Buffer.from(b).toString()),
				},
			);
			await ready.promise;
			for (let i = 0; i < 3; i++)
				connections.push(await vm.network.connect(17171));
			const results = await Promise.all(
				connections.map((connection) =>
					connection.read().then(
						(value) => ({ ok: true, body: Buffer.from(value.body).toString() }),
						(error) => ({ ok: false, error: String(error) }),
					),
				),
			);
			expect(results.slice(0, 2), stderr).toEqual([
				{ ok: true, body: "done" },
				{ ok: true, body: "done" },
			]);
			if (maximum === 2)
				expect(results[2]).toMatchObject({
					ok: false,
					error: expect.stringMatching(/IN_FLIGHT_REQUEST_LIMIT/),
				});
			else expect(results[2], stderr).toEqual({ ok: true, body: "done" });
			for (const connection of connections) await connection.close();
			connections.length = 0;
			await vm.process.kill(proc.pid);
		} finally {
			await vm?.dispose();
			await sidecar.dispose();
		}
	}, 20000);

test("explicit physical control capacity admits 600 real parked guest reads", async () => {
	const sidecar = await AgentOs.createSidecar({
		runtime: {
			protocol: {
				maxInFlightRequests: 1024,
				maxTerminalFrames: 1024,
				maxTerminalBytes: 16 * 1024 * 1024,
				maxControlFrames: 2048,
				maxControlBytes: 64 * 1024 * 1024,
			},
		},
	});
	let vm: AgentOs | undefined;
	const connections: Awaited<ReturnType<AgentOs["network"]["connect"]>>[] = [];
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: {
				resources: {
					maxSockets: 2048,
					maxConnections: 2048,
					maxSocketBufferedBytes: 32 * 1024 * 1024,
				},
			},
		});
		const ready = Promise.withResolvers<void>(),
			accepted = Promise.withResolvers<void>();
		let stderr = "";
		const child = await vm.process.spawn(
			"node",
			[
				"-e",
				`const sockets=[];require('node:net').createServer(socket=>{sockets.push(socket);if(sockets.length===600)console.log('accepted');}).listen(17172,'127.0.0.1',()=>console.log('ready'));process.stdin.on('data',()=>{for(const socket of sockets)socket.end('done');});`,
			],
			{
				streamStdin: true,
				timeoutMs: 30000,
				onStdout: (b) => {
					const text = Buffer.from(b).toString();
					if (text.includes("ready")) ready.resolve();
					if (text.includes("accepted")) accepted.resolve();
				},
				onStderr: (b) => (stderr += Buffer.from(b).toString()),
			},
		);
		await ready.promise;
		for (let index = 0; index < 600; index++)
			connections.push(await vm.network.connect(17172));
		await accepted.promise;
		const reads = connections.map((connection) =>
			connection.read().then(
				(value) => ({ ok: true, body: Buffer.from(value.body).toString() }),
				(error) => ({ ok: false, error: String(error) }),
			),
		);
		await vm.process.writeStdin(child.pid, "release\n");
		const results = await Promise.all(reads);
		expect(results, stderr).toEqual(
			Array.from({ length: 600 }, () => ({ ok: true, body: "done" })),
		);
		for (const connection of connections) await connection.close();
		connections.length = 0;
		await vm.process.kill(child.pid);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 45000);
