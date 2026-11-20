import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("Blob and File retain their public constructor names through runtime bundling", async () => {
	const source = `const blob=new Blob(['bytes']),file=new File(['bytes'],'test.txt');console.log(JSON.stringify({blob:[Blob.name,blob.constructor.name,Object.prototype.toString.call(blob)],file:[File.name,file.constructor.name,Object.prototype.toString.call(file),file.name]}));`;
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
		const p = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 10000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 15000);
