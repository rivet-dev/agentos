import { expect, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";

test("completed synchronous host commands cancel their deadline tasks before VM disposal", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	const errors: unknown[] = [];
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny", hostFunction: "allow" },
			hostFunctions: {
				example: {
					ping: {
						inputSchema: z.object({}).strict(),
						execute: () => ({ value: 42 }),
					},
				},
			},
		});
		let stdout = "",
			stderr = "";
		const process = await vm.process.spawn(
			"node",
			[
				"--input-type=module",
				"-e",
				`
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
for(let n=0;n<2;n++){
 const result=JSON.parse(execFileSync('agentos-example',['ping','--json','{}'],{encoding:'utf8',timeout:60000}));
 assert.deepEqual(result,{ok:true,result:{value:42}});
}
console.log('complete');
`,
			],
			{
				timeoutMs: 10000,
				onStdout: (bytes) => (stdout += Buffer.from(bytes).toString()),
				onStderr: (bytes) => (stderr += Buffer.from(bytes).toString()),
			},
		);
		expect((await vm.process.wait(process.pid)).exitCode, stderr).toBe(0);
		expect(stdout.trim()).toBe("complete");
		// Disposal must release completed commands immediately, rather than await their 60s deadlines.
		await vm.dispose();
		vm = undefined;
	} catch (error) {
		errors.push(error);
	} finally {
		try {
			await vm?.dispose();
		} catch (error) {
			errors.push(error);
		}
		try {
			await sidecar.dispose();
		} catch (error) {
			errors.push(error);
		}
	}
	if (errors.length)
		throw new AggregateError(errors, "synchronous host command cleanup failed");
}, 25000);
