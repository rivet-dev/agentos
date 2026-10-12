import { execFile } from "node:child_process";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { promisify } from "node:util";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const program = `
import urllib.request, urllib.error, json, sys
rows = []
for path in ['/ok', '/error']:
    try:
        response = urllib.request.urlopen(sys.argv[1] + path)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        rows.append([response.read(0).hex(), response.read(2).hex(), response.read(1).hex(), response.read().hex(), response.read().hex()])
    rows.append(['closed', response.read().hex(), response.readinto(b'closed')])
with urllib.request.urlopen(sys.argv[1] + '/ok') as response:
    try:
        response.read1(None)
    except TypeError:
        rows.append(['invalid-read1', 'TypeError'])
    buffer = bytearray(2)
    rows.append(['buffered', response.readinto(buffer), buffer.hex(), response.readline().hex(), [line.hex() for line in response], response.readinto(buffer)])
print(json.dumps(rows))
`;

test("Python HTTP partial reads, EOF, close and error bodies match native CPython", async () => {
	const server = createServer((request, response) => {
		response.writeHead(request.url === "/error" ? 418 : 200);
		response.end(Buffer.from([0, 255, 128, 10, 13, 90]));
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const port = (server.address() as AddressInfo).port;
	const url = `http://127.0.0.1:${port}`;
	let vm: AgentOs | undefined;
	try {
		const native = await promisify(execFile)("python3", ["-c", program, url], {
			timeout: 10_000,
		});
		vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "allow" },
			loopbackExemptPorts: [port],
		});
		const guest = await vm.process.execFile("python", ["-c", program, url], {
			timeoutMs: 30_000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
	} finally {
		await vm?.dispose();
		await new Promise<void>((resolve, reject) =>
			server.close((error) => (error ? reject(error) : resolve())),
		);
	}
}, 60_000);
