import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("V8 Deserializer refuses oversized input before retaining native state", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { reactor: { maxBridgeResponseBytes: 262144 } },
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn(
			"node",
			[
				"-e",
				`try{new(require('node:v8').Deserializer)(Buffer.alloc(262145));throw Error('accepted');}catch(error){console.log(JSON.stringify([error.code,error.configurationPath]));}`,
			],
			{
				timeoutMs: 5000,
				onStdout: (b) => (stdout += Buffer.from(b).toString()),
				onStderr: (b) => (stderr += Buffer.from(b).toString()),
			},
		);
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual([
			"ERR_AGENTOS_BRIDGE_RESPONSE_LIMIT",
			"limits.reactor.maxBridgeResponseBytes",
		]);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 10000);

test("V8 Deserializer retires native owners at each non-retained execution boundary", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { reactor: { maxCapabilities: 512 } },
		});
		for (let call = 0; call < 8; call++) {
			const result = await vm.javascript.evaluate(
				`(()=>{const {DefaultDeserializer,serialize}=require('node:v8'),wire=serialize({answer:42}),readers=[];for(let i=0;i<128;i++)readers.push(new DefaultDeserializer(wire));const reader=readers[0];reader.readHeader();return reader.readValue().answer;})()`,
				{ format: "commonjs", timeoutMs: 5000 },
			);
			expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
			expect(result.value).toBe(42);
		}
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 20000);
