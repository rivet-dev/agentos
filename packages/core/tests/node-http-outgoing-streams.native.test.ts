import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("HTTP uploads preserve binary chunks, early responses and 100-continue", async () => {
	const server = createServer(async (request, response) => {
		try {
			if (request.url === "/early") response.write("ready:");
			const chunks = [];
			for await (const bytes of request) chunks.push(bytes);
			const body = Buffer.concat(chunks);
			response.end(
				JSON.stringify({
					hex: body.toString("hex"),
					chunked: request.headers["transfer-encoding"] === "chunked",
				}),
			);
		} catch {
			response.destroy();
		}
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string") throw Error("missing host port");
	const source = `const http=require('node:http');(async()=>{const results=[];for(const mode of ['early','chunked','continue']){const result=await new Promise((resolve,reject)=>{let text='',continued=false,released=false;const request=http.request('http://127.0.0.1:${address.port}/'+mode,{method:'POST',headers:mode==='chunked'?{}:{'content-length':mode==='early'?2:1,...(mode==='continue'?{expect:'100-continue'}:{})}},response=>{response.on('data',bytes=>{text+=bytes;if(mode==='early'&&!released&&text.startsWith('ready:')){released=true;request.end('b');}});response.on('end',()=>{clearTimeout(timer);resolve({mode,text,continued});});response.on('error',reject);});const timer=setTimeout(()=>request.destroy(Error('outgoing '+mode+' deadline')),4000);request.on('error',error=>{clearTimeout(timer);reject(error);});request.on('continue',()=>{continued=true;request.end(Buffer.from([255]));});if(mode==='continue')request.flushHeaders();else if(mode==='early')request.write('a');else request.write(Buffer.from([255,0]),()=>setTimeout(()=>request.end(Buffer.from([128])),5));});results.push(result);}console.log(JSON.stringify(results));})().catch(error=>{console.error(error.stack);process.exitCode=1;});`;
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		const host = await new Promise<{
			code: number | null;
			stdout: string;
			stderr: string;
		}>((resolve, reject) => {
			const child = spawn(process.execPath, ["-e", source], {
				stdio: ["ignore", "pipe", "pipe"],
			});
			let stdout = "",
				stderr = "";
			const timer = setTimeout(() => child.kill("SIGKILL"), 15000);
			child.stdout.on("data", (b) => (stdout += b));
			child.stderr.on("data", (b) => (stderr += b));
			child.once("error", reject);
			child.once("close", (code) => {
				clearTimeout(timer);
				resolve({ code, stdout, stderr });
			});
		});
		expect(host.code, host.stderr).toBe(0);
		const permissions = {
			network: {
				default: "deny" as const,
				rules: [
					{
						mode: "allow" as const,
						patterns: [`tcp://127.0.0.1:${address.port}`],
					},
				],
			},
		};
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions,
		});
		await vm.configurePermissions(permissions, {
			loopbackExemptPorts: [address.port],
		});
		let stdout = "",
			stderr = "";
		const child = await vm.process.spawn("node", ["-e", source], {
			timeoutMs: 15000,
			onStdout: (b) => (stdout += Buffer.from(b).toString()),
			onStderr: (b) => (stderr += Buffer.from(b).toString()),
		});
		expect((await vm.process.wait(child.pid)).exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout)).toEqual(JSON.parse(host.stdout));
	} finally {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
			server.closeAllConnections();
			await new Promise<void>((resolve) => server.close(() => resolve()));
		}
	}
}, 25000);
