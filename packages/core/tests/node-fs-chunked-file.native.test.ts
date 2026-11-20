import { spawnSync } from "node:child_process";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs, createHostDirBackend } from "../src/index.js";

test("async file APIs transfer files larger than bridge frames while retaining read-only mounts and closing descriptors", async () => {
	const directory = await mkdtemp(join(tmpdir(), "agentos-chunked-file-"));
	const input = join(directory, "input");
	await writeFile(input, "€".repeat(2_100_000));
	await chmod(input, 0o444);
	const source = `import{readFile,writeFile}from'node:fs/promises';import{openSync,closeSync}from'node:fs';
const control=new Promise(resolve=>process.stdin.once('data',chunk=>resolve(chunk.toString())));process.stdin.resume();await new Promise(resolve=>setTimeout(resolve,20));
const[input,output]=process.argv.slice(1);const before=openSync(output,'w');closeSync(before);
const data=await readFile(input),text=await readFile(input,'utf8');await writeFile(output,data);
const copy=await readFile(output);let denied=false;try{await writeFile(input,'changed');}catch{denied=true;}
let missing;try{await readFile(output+'-missing');}catch(e){missing=e.code;}
const controller=new AbortController();controller.abort();let aborted;try{await readFile(input,{signal:controller.signal});}catch(e){aborted=e.code;}
const after=openSync(output,'r');closeSync(after);
console.log(JSON.stringify({bytes:data.length,characters:text.length,equal:data.equals(copy),original:data.equals(await readFile(input)),denied,missing,aborted,descriptorsClosed:before===after}));console.log(JSON.stringify({stdin:await control}));process.stdin.pause();`;
	const host = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source, input, join(directory, "output")],
		{ input: "control", encoding: "utf8", timeout: 10000, maxBuffer: 16384 },
	);
	expect(host.status, host.stderr).toBe(0);
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			user: {
				uid: process.getuid!(),
				gid: process.getgid!(),
				username: "test",
				homedir: "/home/test",
			},
			permissions: { network: "deny" },
			limits: {
				reactor: {
					maxBridgeRequestBytes: 1024 * 1024,
					maxBridgeResponseBytes: 4 * 1024 * 1024,
				},
			},
			mounts: [
				{
					path: "/input",
					readOnly: true,
					plugin: createHostDirBackend({ hostPath: directory, readOnly: true }),
				},
			],
		});
		let stdout = "",
			stderr = "";
		const ready = Promise.withResolvers<void>();
		const child = await vm.process.spawn(
			"node",
			["--input-type=module", "-e", source, "/input/input", "/tmp/output"],
			{
				timeoutMs: 10000,
				streamStdin: true,
				onStdout: (c) => {
					stdout += Buffer.from(c).toString();
					if (stdout.includes("\n")) ready.resolve();
				},
				onStderr: (c) => {
					stderr += Buffer.from(c).toString();
				},
			},
		);
		const exit = vm.process.wait(child.pid);
		await Promise.race([
			ready.promise,
			exit.then((result) => {
				throw new Error(
					`Guest exited before file reads completed (${result.exitCode}): ${stderr}`,
				);
			}),
		]);
		await vm.process.writeStdin(child.pid, "control");
		await vm.process.closeStdin(child.pid);
		expect((await exit).exitCode, stderr).toBe(0);
		expect(
			stdout
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line)),
		).toEqual(
			host.stdout
				.trim()
				.split("\n")
				.map((line) => JSON.parse(line)),
		);
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
			await rm(directory, { recursive: true, force: true });
		}
	}
}, 20000);
