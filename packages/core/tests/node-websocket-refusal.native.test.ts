import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("WebSocket preserves an immediate peer close frame and reason after upgrade", async () => {
	let connections = 0;
	const server = createServer((socket) => {
		connections++;
		let head = "";
		socket.setTimeout(5000, () => socket.destroy());
		socket.on("error", (error) =>
			console.error("refusal fixture socket", error),
		);
		socket.on("data", (bytes) => {
			head += bytes.toString("latin1");
			if (head.length > 8192) {
				socket.destroy(new Error("refusal fixture headers exceed8192 bytes"));
				return;
			}
			if (!head.includes("\r\n\r\n")) return;
			socket.removeAllListeners("data");
			const key = /sec-websocket-key: *(\S+)/i.exec(head)?.[1];
			if (!key) {
				socket.destroy(new Error("missing websocket key"));
				return;
			}
			const accept = createHash("sha1")
				.update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
				.digest("base64");
			const payload = Buffer.concat([
				Buffer.from([0x03, 0xf3]),
				Buffer.from("actor.not_found#synthetic-ray"),
			]);
			socket.end(
				Buffer.concat([
					Buffer.from(
						`HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: ${accept}\r\nSec-WebSocket-Protocol: rivet\r\n\r\n`,
					),
					Buffer.from([0x88, payload.length]),
					payload,
				]),
			);
		});
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string") throw Error("missing peer");
	const source = `const socket=new WebSocket('ws://127.0.0.1:${address.port}/',['rivet']);const result=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('peer close timeout')),4000);socket.addEventListener('close',event=>{clearTimeout(timer);resolve({code:event.code,reason:event.reason});});socket.addEventListener('error',event=>reject(Error(event.message??'websocket error')));});console.log(JSON.stringify(result));`;
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		const host = spawn(
			process.execPath,
			["--input-type=module", "-e", source],
			{ stdio: ["ignore", "pipe", "pipe"] },
		);
		let hostOut = "",
			hostErr = "";
		host.stdout.on("data", (b) => (hostOut += b));
		host.stderr.on("data", (b) => (hostErr += b));
		const code = await new Promise<number | null>((resolve, reject) => {
			host.on("error", reject);
			host.on("exit", resolve);
		});
		expect(code, hostErr).toBe(0);
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: {
				network: {
					default: "deny",
					rules: [
						{ mode: "allow", patterns: [`tcp://127.0.0.1:${address.port}`] },
					],
				},
			},
			loopbackExemptPorts: [address.port],
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
		expect(JSON.parse(hostOut)).toEqual({
			code: 1011,
			reason: "actor.not_found#synthetic-ray",
		});
		expect(JSON.parse(stdout)).toEqual(JSON.parse(hostOut));
		expect(connections).toBe(2);
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
			await new Promise<void>((resolve, reject) =>
				server.close((e) => (e ? reject(e) : resolve())),
			);
		}
	}
}, 15000);
