import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// Inject a transport error at writable finish, after HTTP parsing has detached
// but before socket close. This deterministically exercises pending-write error
// ownership without depending on TCP packet timing or a remote peer race.
const source = String.raw`const http = require('node:http');
const net = require('node:net');
const custom = CUSTOM_HANDLER;
const result = { clientError: null, sameSocket: false, closed: false };
const failure = Object.assign(new Error('synthetic write failure'), {code:'EPIPE'});
let accepted;
const server = http.createServer((req, res) => res.end('ok'));
if (custom) server[custom === 'once' ? 'once' : 'on']('clientError', (error, socket) => {
 result.clientError = error === failure && error.code;
 result.sameSocket = socket === accepted;
 socket.destroy();
});
server.on('connection', socket => {
 accepted = socket;
 socket.once('finish', () => socket.emit('error', failure));
 socket.once('close', () => {
  result.closed = true;
  server.close(() => console.log(JSON.stringify(result)));
 });
});
server.listen(0, '127.0.0.1', () => {
 const client = net.connect(server.address().port, '127.0.0.1', () => client.end('GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n'));
 client.resume();
});
`;

for (const customHandler of [false, true, "once"]) {
	test(`HTTP retains socket error ownership through finish (clientError handler: ${customHandler})`, async () => {
		const program = source.replace(
			"CUSTOM_HANDLER",
			JSON.stringify(customHandler),
		);
		const native = spawnSync(process.execPath, ["-e", program], {
			encoding: "utf8",
			timeout: 5000,
			maxBuffer: 4096,
		});
		expect(native.status, native.error?.message ?? native.stderr).toBe(0);
		expect(native.stderr).toBe("");
		const expected = JSON.parse(native.stdout);
		expect(expected).toEqual({
			clientError: customHandler ? "EPIPE" : null,
			sameSocket: Boolean(customHandler),
			closed: true,
		});
		const vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: {
				fs: "allow",
				process: "allow",
				childProcess: "allow",
				network: "allow",
			},
		});
		try {
			const actual = await vm.process.execFile("node", ["-e", program], {
				timeoutMs: 5000,
				output: { capture: "all", maxBufferedBytes: 4096 },
			});
			expect(actual.exitCode, actual.stderr).toBe(0);
			expect(actual.stderr).toBe("");
			expect(JSON.parse(actual.stdout)).toEqual(expected);
		} finally {
			await vm.dispose();
		}
	}, 15000);
}
