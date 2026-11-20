import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

for (const format of ["commonjs", "module"] as const)
	test(`node:process ${format} stdio remains initialized across the console import cycle`, async () => {
		const source =
			(format === "module"
				? `import process,{stdout,stderr}from'node:process';if(stdout!==process.stdout||stderr!==process.stderr)throw Error('module stdio differs');`
				: `const process=require('node:process');`) +
			`if(process!==globalThis.process)throw Error('process identity');process.stdout.write('out\\n',()=>{process.stderr.write('err\\n');console.log(JSON.stringify({stdout:process.stdout.fd,stderr:process.stderr.fd,same:process.stdout===process.stdout}));});`;
		const host = spawnSync(
			process.execPath,
			["--input-type=" + format, "-e", source],
			{ encoding: "utf8", timeout: 5000 },
		);
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
			const child = await vm.process.spawn(
				"node",
				["--input-type=" + format, "-e", source],
				{
					timeoutMs: 5000,
					onStdout: (b) => (stdout += Buffer.from(b).toString()),
					onStderr: (b) => (stderr += Buffer.from(b).toString()),
				},
			);
			expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
			expect(stdout).toBe(host.stdout);
			expect(stderr).toBe(host.stderr);
		} finally {
			await vm?.dispose();
			await sidecar.dispose();
		}
	}, 12000);
