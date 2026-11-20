import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const hostExec = promisify(execFile);
async function compare(source: string) {
	const expected = (await hostExec(process.execPath, ["-e", source])).stdout;
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
		const processHandle = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 25000,
			onStdout: (b) => {
				stdout += Buffer.from(b).toString();
			},
			onStderr: (b) => {
				stderr += Buffer.from(b).toString();
			},
		});
		expect((await vm.process.wait(processHandle.pid)).exitCode, stderr).toBe(0);
		expect(stdout).toBe(expected);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}
test("a fired one-shot timer can refresh itself, while a cleared timer stays cleared", async () => {
	await compare(
		`let count=0;const timer=setTimeout(()=>{console.log(++count);if(count<2)timer.refresh();},1);const cleared=setTimeout(()=>{throw Error('cleared timer fired');},100);clearTimeout(cleared);cleared.refresh();`,
	);
}, 30000);
test("cleared timer handles release their quota before the next admission", async () => {
	await compare(
		`for(let i=0;i<5000;i++){const t=setTimeout(()=>{throw Error('cleared timer fired');},60000);clearTimeout(t);}setTimeout(()=>console.log('retired'),1);`,
	);
}, 30000);

test("refresh preserves callback identity and cancellation through numeric handles", async () => {
	await compare(
		`let count=0;const timer=setTimeout(function(){const id=+this;if(++count===1){this.refresh();console.log(id===+this);clearTimeout(id);}else throw Error('cancelled refreshed timer fired');},1);setTimeout(()=>console.log('cancelled'),20);`,
	);
}, 30000);
test("an expired timer can refresh later and preserves its reference state", async () => {
	await compare(
		`let count=0;const timer=setTimeout(()=>{console.log(++count);},1);timer.unref();setTimeout(()=>{timer.refresh();console.log(timer.hasRef());timer.ref();},20);`,
	);
}, 30000);
