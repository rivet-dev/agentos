import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function execute(source: string, heapMb = 128) {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { jsRuntime: { v8HeapLimitMb: heapMb } },
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 15000,
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

test("large Uint8Array encodings preserve offset/shared views and every base64 remainder", async () => {
	const source = `const{createHash}=require('node:crypto');const observations=[],digest=s=>[s.length,createHash('sha256').update(s).digest('hex')];for(const shared of[false,true])for(const length of[65536,65537,65538,65539]){const buffer=shared?new SharedArrayBuffer(length+8):new ArrayBuffer(length+8),whole=new Uint8Array(buffer);for(let i=0;i<whole.length;i++)whole[i]=i%251;const view=whole.subarray(3,length+3);observations.push([shared,length,digest(hex(view)),digest(base64(view)),digest(base64(view,{omitPadding:true})),digest(base64(view,{alphabet:'base64url'})),digest(base64(view,{alphabet:'base64url',omitPadding:true}))]);}console.log(JSON.stringify(observations));`;
	// Node 24's experimental Uint8Array encoder ignores view offsets. Buffer
	// provides the established reference for these exact bytes and options.
	const reference = `const hex=v=>Buffer.from(v).toString('hex'),base64=(v,o={})=>{let s=Buffer.from(v).toString('base64');if(o.alphabet==='base64url')s=s.replaceAll('+','-').replaceAll('/','_');return o.omitPadding?s.replace(/=+$/,''):s;};`;
	const guest = `const hex=v=>v.toHex(),base64=(v,o)=>v.toBase64(o);`;
	const host = spawnSync(process.execPath, ["-e", reference + source], {
		encoding: "utf8",
		timeout: 15000,
	});
	expect(host.status, host.stderr).toBe(0);
	expect(await execute(guest + source)).toEqual(JSON.parse(host.stdout));
}, 25000);

test("Uint8Array encoding refuses its external-memory budget before allocating and keeps the VM usable", async () => {
	const source = `const bytes=new Uint8Array(40*1024*1024);let refusal;try{bytes.toHex();throw Error('encoding ignored quota');}catch(error){refusal={name:error.name,code:error.code};}console.log(JSON.stringify({refusal,after:new Uint8Array([1,255]).toHex(),inputBytes:bytes.byteLength}));`;
	expect(await execute(source, 64)).toEqual({
		refusal: {
			name: "RangeError",
			code: "ERR_AGENTOS_V8_EXTERNAL_MEMORY_LIMIT",
		},
		after: "01ff",
		inputBytes: 40 * 1024 * 1024,
	});
}, 20000);
