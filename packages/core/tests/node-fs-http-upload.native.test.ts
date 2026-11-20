import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("filesystem streams finish real HTTP uploads for empty, small, ranged and large files", async () => {
	const server = createServer(async (request, response) => {
		if (request.url === "/download") {
			response.end(Buffer.alloc(512 * 1024 - 1, 37));
			return;
		}
		if (request.method === "HEAD") {
			response.setHeader("content-length", "0");
			response.end();
			return;
		}
		const hash = createHash("md5");
		let size = 0;
		try {
			for await (const bytes of request) {
				size += bytes.length;
				hash.update(bytes);
			}
		} catch {
			response.destroy();
			return;
		}
		response.end(JSON.stringify({ size, md5: hash.digest("hex") }));
	});

	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string") throw Error("missing host port");
	const source = `const fs=require('node:fs'),http=require('node:http'),os=require('node:os'),path=require('node:path'),crypto=require('node:crypto');(async()=>{const dir=fs.mkdtempSync(path.join(os.tmpdir(),'fs-http-')),agent=new http.Agent({keepAlive:true,maxSockets:1});try{const results=[];for(const size of [0,1,65537,1024*1024]){const file=path.join(dir,'body'),body=Buffer.alloc(size,37);fs.writeFileSync(file,body);const options=size===1?{start:0,end:0}:{};const result=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>request.destroy(Error('upload deadline at size '+size)),4000);const request=http.request('http://127.0.0.1:${address.port}/',{agent,method:'POST',headers:{'content-length':size}},response=>{let text='';response.on('data',b=>text+=b);response.on('end',()=>{clearTimeout(timer);resolve(JSON.parse(text));});response.on('error',reject);});request.on('error',error=>{clearTimeout(timer);reject(error);});fs.createReadStream(file,options).on('error',reject).pipe(request);});await new Promise((resolve,reject)=>{const request=http.request('http://127.0.0.1:${address.port}/',{agent,method:'HEAD'},response=>{response.resume();response.on('end',resolve);});request.on('error',reject);request.end();});if(size===0)await new Promise((resolve,reject)=>{const download=http.get('http://127.0.0.1:${address.port}/download',{agent},async response=>{try{let bytes=0;for await(const chunk of response){bytes+=chunk.length;await new Promise(resolve=>setTimeout(resolve,1));}bytes===512*1024-1?resolve():reject(Error('truncated download'));}catch(error){reject(error);}});download.on('error',reject);});results.push({size,result,expected:crypto.createHash('md5').update(body).digest('hex')});}console.log(JSON.stringify(results));}finally{agent.destroy();fs.rmSync(dir,{recursive:true,force:true});}})().catch(error=>{console.error(error.stack);process.exitCode=1;});`;
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
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: {
				network: {
					default: "deny",
					rules: [
						{ mode: "allow", patterns: [`tcp://127.0.0.1:${address.port}`] },
					],
				},
			},
		});
		await vm.configurePermissions(
			{
				network: {
					default: "deny",
					rules: [
						{ mode: "allow", patterns: [`tcp://127.0.0.1:${address.port}`] },
					],
				},
			},
			{ loopbackExemptPorts: [address.port] },
		);
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
