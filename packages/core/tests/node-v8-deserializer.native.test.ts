import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("V8 Deserializer retains its cursor, graph references and subclass host-object validation", async () => {
	const source = `const v8=require('node:v8');const shared={value:7};const encoded=v8.serialize({a:shared,b:shared,bytes:Buffer.from([1,2,3])});const normal=new v8.DefaultDeserializer(encoded);const header=normal.readHeader(),value=normal.readValue();
 class Reader extends v8.Deserializer{_readHostObject(){const tag=this.readUint32();if(tag!==4)throw new Error('rejected tag '+tag);const size=this.readUint32();if(size>4)throw new Error('bounded host length');return{tag,text:this.readRawBytes(size).toString('utf8')};}}
 const custom=new Reader(Buffer.from([255,15,92,4,2,104,105]));custom.readHeader();const host=custom.readValue();const bad=new Reader(Buffer.from([255,15,92,4,128,8]));bad.readHeader();let error;try{bad.readValue();}catch(e){error=e.message;}
 console.log(JSON.stringify({header,same:value.a===value.b,buffer:Buffer.isBuffer(value.bytes),bytes:[...value.bytes],host,error,version:normal.getWireFormatVersion()>0}));`;
	const reference = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(reference.status, reference.stderr).toBe(0);
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
		expect(JSON.parse(stdout)).toEqual(JSON.parse(reference.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 10000);
