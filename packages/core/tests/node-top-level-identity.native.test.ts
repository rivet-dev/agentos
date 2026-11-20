import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("successive top-level Node processes use their live kernel identities", async () => {
	const source = `import {pid,ppid} from 'node:process';import {createRequire} from 'node:module';
const cjs=createRequire(import.meta.url)('node:process');let alive;
try{alive=process.kill(process.pid,0);}catch(error){alive=error.code;}
console.log(JSON.stringify({pid,ppid,alive,same:pid===process.pid&&cjs.pid===pid&&cjs.ppid===ppid}));`;
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 3000, maxBuffer: 16384 },
	);
	expect(host.status, host.stderr).toBe(0);
	const reference = JSON.parse(host.stdout);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		let previous = 0;
		for (let index = 0; index < 2; index++) {
			let stdout = "",
				stderr = "";
			const proc = await vm.process.spawn(
				"node",
				["--input-type=module", "-e", source],
				{
					timeoutMs: 3000,
					onStdout: (c) => {
						stdout += Buffer.from(c).toString();
					},
					onStderr: (c) => {
						stderr += Buffer.from(c).toString();
					},
				},
			);
			expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
			const actual = JSON.parse(stdout);
			expect(actual.alive).toBe(reference.alive);
			expect(actual.same).toBe(reference.same);
			expect(actual.pid).toBeGreaterThan(previous);
			expect(actual.ppid).toBe(0);
			previous = actual.pid;
		}
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 10000);
