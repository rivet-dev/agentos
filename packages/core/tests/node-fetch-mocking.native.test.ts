import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

test("fetch supports Node-style mocks while saved network entrypoints retain policy", async () => {
	const source = `const original=globalThis.fetch;let calls=0;
const mock=async()=>{calls++;return new Response('mocked');};
globalThis.fetch=mock;
const assigned=globalThis.fetch===mock;
const body=await fetch('http://example.invalid/').then(r=>r.text(),e=>e.code??e.message);
globalThis.fetch=original;
console.log(JSON.stringify({assigned,body,calls,restored:globalThis.fetch===original}));`;
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 5000, maxBuffer: 16384 },
	);
	expect(host.status, host.stderr).toBe(0);
	let hostRequests = 0;
	const server = createServer((_request, response) => {
		hostRequests++;
		response.end("host");
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string")
		throw new Error("Missing host listener");
	const sidecar = await AgentOs.createSidecar();
	const modules = resolve(
		fileURLToPath(new URL("../../../node_modules", import.meta.url)),
	);
	const require = createRequire(
		new URL("../../build-tools/package.json", import.meta.url),
	);
	const undici = pathToFileURL(require.resolve("undici")).href;
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			mounts: [
				{
					path: modules,
					readOnly: true,
					plugin: createHostDirBackend({ hostPath: modules, readOnly: true }),
				},
			],
		});
		let stdout = "",
			stderr = "";
		const policy = `
const target='http://127.0.0.1:${address.port}/';
const {fetch:transportFetch}=await import(${JSON.stringify(undici)});
const http=await import('node:http');
const blocked=[];
for(const operation of [()=>original(target),()=>transportFetch(target),()=>new Promise((r,j)=>{const q=http.get(target,r);q.once('error',j);})]){
 try{await operation();blocked.push(false);}catch{blocked.push(true);}
}console.log(JSON.stringify({blocked}));`;
		const proc = await vm.process.spawn(
			"node",
			["--input-type=module", "-e", source + policy],
			{
				timeoutMs: 8000,
				onStdout: (c) => {
					stdout += Buffer.from(c).toString();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
				},
			},
		);
		expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
		const rows = stdout
			.trim()
			.split("\n")
			.map((line) => JSON.parse(line));
		expect(rows[0]).toEqual(JSON.parse(host.stdout));
		expect(rows[1]).toEqual({ blocked: [true, true, true] });
		expect(hostRequests).toBe(0);
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
			await new Promise<void>((resolve, reject) =>
				server.close((error) => (error ? reject(error) : resolve())),
			);
		}
	}
}, 15000);
