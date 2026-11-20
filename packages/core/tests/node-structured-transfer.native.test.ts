import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("structuredClone detaches transferred ArrayBuffers after complete graph validation", async () => {
	const source = `const buffer=new ArrayBuffer(4);new Uint8Array(buffer).set([1,2,3,4]);const value={buffer,view:new Uint8Array(buffer,1,2),data:new DataView(buffer,2,2)};value.self=value;const copy=structuredClone(value,{transfer:[buffer]});const result={detached:buffer.byteLength,bytes:[...copy.view],data:copy.data.getUint8(0),same:copy.buffer===copy.view.buffer&&copy.buffer===copy.data.buffer,cycle:copy.self===copy};result.errors=[];
 for(const kind of ['duplicate','invalid','clone-failure']){const original=new ArrayBuffer(2);let name;try{const transfer=kind==='duplicate'?[original,original]:kind==='invalid'?[original,{}]:[original];structuredClone(kind==='clone-failure'?()=>{}:original,{transfer});}catch(error){name=error.name;}result.errors.push([kind,name,original.byteLength]);}
 const absent=new ArrayBuffer(2);result.absent=[structuredClone(42,{transfer:[absent]}),absent.byteLength];console.log(JSON.stringify(result));`;
	const host = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 5000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 10000);
