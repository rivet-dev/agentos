import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("Buffer byte views and Node adapters for Web Streams match Node", async () => {
	const source = `import {Readable,Writable} from 'node:stream';
const backing=new Uint8Array([9,1,2,9]);const visible=backing.subarray(1,3);
const equal=Buffer.from([1,2]).equals(visible);let rejected=false;try{Buffer.from([1]).equals([1]);}catch{rejected=true;}
const input=new ReadableStream({start(c){c.enqueue(visible);c.enqueue(new Uint8Array([3]));c.close();}});
const chunks=[];for await(const chunk of Readable.fromWeb(input))chunks.push(chunk);
const written=[];let closed=false;const output=Writable.fromWeb(new WritableStream({async write(c){await Promise.resolve();written.push([...c]);},close(){closed=true;}}));
await new Promise((resolve,reject)=>{output.once('error',reject);output.end(Buffer.from([4,5]),resolve);});
let canceled=false;const abandoned=Readable.fromWeb(new ReadableStream({cancel(){canceled=true;}}));
await new Promise(resolve=>{abandoned.once('close',resolve);abandoned.destroy();});
console.log(JSON.stringify({equal,rejected,read:[...Buffer.concat(chunks)],written,closed,canceled}));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source], { encoding: "utf8", timeout: 5000, maxBuffer: 16384 });
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar(); let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: { network: "deny" } });
		let stdout = "", stderr = "";
		const process = await vm.process.spawn("node", ["--input-type=module", "-e", source], { timeoutMs: 5000, onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); } });
		expect((await vm.process.wait(process.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); } }
}, 10000);
