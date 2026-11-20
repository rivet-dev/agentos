import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function execute(source: string, maxBlobBytes?: number) {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: {
				jsRuntime: {
					v8HeapLimitMb: 256,
					...(maxBlobBytes === undefined ? {} : { maxBlobBytes }),
				},
			},
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 20000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		return JSON.parse(stdout);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}

test("Blob and File retain the default64MiB limit and cannot change trusted policy from guest globals", async () => {
	const source = `globalThis.__agentOSBridgeLimits={maxBlobBytes:256*1024*1024};globalThis.__agentOSBridgeLimitInstaller=()=>{};const bytes=new Uint8Array(65*1024*1024),refusals=[];for(const make of[()=>new Blob([bytes]),()=>new File([bytes],'large')]){try{make();throw Error('Blob ignored default quota');}catch(error){refusals.push([error.name,error.code,/limits.jsRuntime.maxBlobBytes/.test(error.message)]);}}console.log(JSON.stringify({refusals,usable:new Blob(['ok']).size}));`;
	expect(await execute(source)).toEqual({
		refusals: [
			["RangeError", "ERR_BLOB_SIZE_LIMIT", true],
			["RangeError", "ERR_BLOB_SIZE_LIMIT", true],
		],
		usable: 2,
	});
}, 25000);

test("trusted Blob byte override preserves bytes, slicing and File inheritance in a real guest", async () => {
	const source = `(async()=>{const bytes=new Uint8Array(65*1024*1024).fill(7),blob=new Blob([bytes,'end']),file=new File([blob],'large.bin');bytes[bytes.length-1]=9;const copy=new Uint8Array(await blob.arrayBuffer());console.log(JSON.stringify({size:blob.size,tail:await blob.slice(-4).text(),length:copy.length,first:copy[0],lastByte:copy[bytes.length-1],fileBlob:file instanceof Blob,fileTail:await file.slice(-4).text(),fileName:file.name}));})().catch(error=>{console.error(error);process.exitCode=1;});`;
	const host = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 20000,
	});
	expect(host.status, host.stderr).toBe(0);
	expect(await execute(source, 96 * 1024 * 1024)).toEqual(
		JSON.parse(host.stdout),
	);
}, 30000);

test("trusted Blob override cannot exceed the configured VM heap budget", async () => {
	const sidecar = await AgentOs.createSidecar();
	try {
		await expect(
			AgentOs.create({
				sidecar: { kind: "explicit", handle: sidecar },
				defaultSoftware: false,
				permissions: { network: "deny" },
				limits: {
					jsRuntime: { v8HeapLimitMb: 128, maxBlobBytes: 129 * 1024 * 1024 },
				},
			}),
		).rejects.toThrow(/maxBlobBytes/);
	} finally {
		await sidecar.dispose();
	}
}, 15000);
