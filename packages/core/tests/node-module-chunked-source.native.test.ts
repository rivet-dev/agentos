import { spawnSync } from "node:child_process";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

test("module imports span bounded bridge responses, preserve UTF-8 and enforce file permissions", async () => {
	const directory = await mkdtemp(join(tmpdir(), "agentos-module-source-"));
	await writeFile(join(directory, "allowed.mjs"), `/*${"€".repeat(700_000)}*/\nexport const value=42;export default '終';\n`);
	await writeFile(join(directory, "denied.mjs"), "export default 'private';");
	await chmod(join(directory, "denied.mjs"), 0);
	const source = `import{pathToFileURL}from'node:url';const root=process.argv[1];
const module=await import(pathToFileURL(root+'/allowed.mjs').href);let denied=false;
const builtin=await import('node:process');const bare=await import('process');
if(builtin.default!==process||bare.default!==process)throw Error('dynamic builtin identity');
try{await import(pathToFileURL(root+'/denied.mjs').href);}catch(e){denied=e.code==='EACCES'||/EACCES|permission denied/i.test(e.message);}
console.log(JSON.stringify({value:module.value,text:module.default,denied}));`;
	const host = spawnSync(process.execPath, ["--input-type=module", "-e", source, directory], { encoding: "utf8", timeout: 10000, maxBuffer: 16384 });
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar(); let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false,
			user: { uid: process.getuid!(), gid: process.getgid!(), username: "test", homedir: "/home/test" }, permissions: { network: "deny" },
			limits: { reactor: { maxBridgeRequestBytes: 1024 * 1024, maxBridgeResponseBytes: 1024 * 1024 } },
			mounts: [{ path: "/input", readOnly: true, plugin: createHostDirBackend({ hostPath: directory, readOnly: true }) }],
		});
		let stdout = "", stderr = "";
		const child = await vm.process.spawn("node", ["--input-type=module", "-e", source, "/input"], { timeoutMs: 10000,
			onStdout: c => { stdout += Buffer.from(c).toString(); }, onStderr: c => { stderr += Buffer.from(c).toString(); },
		});
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally { try { await vm?.dispose(); } finally { await sidecar.dispose(); await rm(directory, { recursive: true, force: true }); } }
}, 20000);
