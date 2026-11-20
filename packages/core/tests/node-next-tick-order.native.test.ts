import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

async function compare(source: string, args: string[] = []) {
	const host = spawnSync(process.execPath, [...args, "-e", source], {
		encoding: "utf8",
		timeout: 5000,
		maxBuffer: 16384,
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
		const p = await vm.process.spawn("node", [...args, "-e", source], {
			timeoutMs: 5000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(p.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}

const cases = [
	{
		name: "CJS top level",
		args: [],
		source: `const out=[];Promise.resolve().then(()=>out.push('promise'));process.nextTick(()=>out.push('tick'));setImmediate(()=>console.log(JSON.stringify(out)));`,
	},
	{
		name: "ESM top level",
		args: ["--input-type=module"],
		source: `const out=[];process.nextTick(()=>out.push('tick'));Promise.resolve().then(()=>out.push('promise'));setImmediate(()=>console.log(JSON.stringify(out)));`,
	},
	{
		name: "inside Promise checkpoint",
		args: [],
		source: `const out=[];Promise.resolve().then(()=>{process.nextTick(()=>out.push('tick'));Promise.resolve().then(()=>out.push('promise')).then(()=>out.push('then'));});setImmediate(()=>console.log(JSON.stringify(out)));`,
	},
	{
		name: "timer callback",
		args: [],
		source: `const out=[];setTimeout(()=>{Promise.resolve().then(()=>out.push('promise'));process.nextTick(()=>out.push('tick'));setImmediate(()=>console.log(JSON.stringify(out)));},1);`,
	},
];

test.each(cases)("Node nextTick ordering: $name", async ({ args, source }) => {
	await compare(source, args);
}, 10000);
test("failed retained calls discard nextTick callbacks and admit a fresh phase", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny", fs: "allow" },
		});
		await vm.createContext("next-ticks");
		const failed = await vm.javascript.execute(
			`(()=>{require('node:fs').writeFileSync('/tmp/entered-next-tick','entered');process.nextTick(()=>require('node:fs').writeFileSync('/tmp/stale-next-tick','stale'));throw Error('stop before tick phase');})()`,
			{ contextId: "next-ticks", format: "commonjs", timeoutMs: 2000 },
		);
		expect(failed.outcome).toBe("failed");
		expect(
			Buffer.from(
				await vm.filesystem.readFile("/tmp/entered-next-tick"),
			).toString("utf8"),
		).toBe("entered");
		expect(await vm.filesystem.exists("/tmp/stale-next-tick")).toBe(false);
		const fresh = await vm.javascript.evaluate(
			`new Promise(resolve=>process.nextTick(()=>resolve(42)))`,
			{ contextId: "next-ticks", format: "commonjs", timeoutMs: 2000 },
		);
		expect(fresh.outcome, JSON.stringify(fresh)).toBe("succeeded");
		expect(fresh.value).toBe(42);
		expect(await vm.filesystem.exists("/tmp/stale-next-tick")).toBe(false);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 10000);
