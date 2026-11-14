import { createServer } from "node:http";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import type { Permissions } from "../src/runtime-compat.js";

test("trusted policy replacement grants and revokes a host peer while preserving other scopes", async () => {
	let firstRequests = 0, secondRequests = 0;
	const first = createServer((_q, s) => { firstRequests++; s.setHeader("connection", "close"); s.end("allowed"); });
	const second = createServer((_q, s) => { secondRequests++; s.end("ungranted"); });
	await Promise.all([first, second].map(server => new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve))));
	const a = first.address(), b = second.address();
	if (!a || typeof a === "string" || !b || typeof b === "string") throw Error("missing host ports");
	const policy = (): Permissions => ({ network: { default: "deny", rules: [{mode:"allow",patterns:["tcp://127.0.0.1:*"]}] }, fs: {default:"allow",rules:[{mode:"deny",paths:["/tmp/blocked"],operations:["read"]}]}, env: "allow", process: "allow", childProcess: "allow", hostFunction: "deny" });
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ sidecar: { kind: "explicit", handle: sidecar }, defaultSoftware: false, permissions: {...policy(),fs:"allow"} });
        await vm.filesystem.writeFile("/tmp/blocked","private");
        await vm.configurePermissions(policy(),{loopbackExemptPorts:[]});
		const ready = Promise.withResolvers<void>();
		let stderr = "";
		const proc = await vm.process.spawn("node", ["--input-type=module", "-e", `
import http from 'node:http';import fs from 'node:fs';
http.createServer(async(req,res)=>{
 let fsCode='';try{fs.readFileSync('/tmp/blocked');}catch(e){fsCode=e.code;}
 let result;try{const response=await fetch('http://127.0.0.1:'+req.url.slice(1)+'/');result={ok:true,body:await response.text(),fsCode};}catch(e){result={ok:false,fsCode,error:String(e),cause:String(e.cause)};}
 res.setHeader('connection','close');res.end(JSON.stringify(result));
}).listen(17171,'127.0.0.1',()=>console.log('ready'));`], {
			timeoutMs: 12000, onStdout: bytes => { if (Buffer.from(bytes).toString().includes("ready")) ready.resolve(); }, onStderr: bytes => { stderr += Buffer.from(bytes).toString(); },
		});
		await ready.promise;
		const probe = async (port: number) => {
			const connection = await vm!.network.connect(17171);
			try {
				await connection.write(Buffer.from(`GET /${port} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n`));
				const chunks: Uint8Array[] = [];
				for (;;) { const chunk = await connection.read(); chunks.push(chunk.body); if (chunk.done) break; }
				const text = Buffer.concat(chunks).toString();
				const result = JSON.parse(text.slice(text.indexOf("\r\n\r\n") + 4));
				expect(result.fsCode, stderr).toMatch(/EACCES|EPERM/);
				return result;
			} finally { await connection.close(); }
		};
		expect((await probe(a.port)).ok).toBe(false);
		await vm.configurePermissions(policy(), { loopbackExemptPorts: [a.port] });
		const granted=await probe(a.port);expect(granted,JSON.stringify(granted)).toMatchObject({ ok: true, body: "allowed" });
		expect((await probe(b.port)).ok).toBe(false);
		await vm.configurePermissions(policy(), { loopbackExemptPorts: [] });
		expect((await probe(a.port)).ok).toBe(false);
		expect(firstRequests).toBe(1); expect(secondRequests).toBe(0);
		await vm.process.kill(proc.pid);
	} finally {
		try { await vm?.dispose(); } finally {
			await sidecar.dispose();
			for (const server of [first, second]) { server.closeAllConnections(); await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve())); }
		}
	}
}, 15000);
