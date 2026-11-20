import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("URL domain helpers normalize SMTP and international host names without accepting credentials or ports", async () => {
	const source = `import {domainToASCII,domainToUnicode} from 'node:url';
const values=['EXAMPLE.COM','localhost','bücher.example','mañana.com','xn--bcher-kva.example','example.com/path','user@example.com','example.com:80','%65xample.com','bad host','[::1]','127.1','',undefined,null,42];
const rows=values.map(value=>[domainToASCII(value),domainToUnicode(value)]);
for(const fn of [domainToASCII,domainToUnicode]){try{fn();rows.push('accepted');}catch(e){rows.push([e.name,e.code]);}}
console.log(JSON.stringify(rows));`;
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{ encoding: "utf8", timeout: 5000, maxBuffer: 16384 },
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
			["--input-type=module", "-e", source],
			{
				timeoutMs: 5000,
				onStdout: (c) => {
					stdout += Buffer.from(c).toString();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
				},
			},
		);
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	}
}, 10000);
