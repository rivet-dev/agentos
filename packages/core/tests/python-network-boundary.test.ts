import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test.each([
	{ name: "default policy", permissions: {} },
	{ name: "network deny", permissions: { network: "deny" as const } },
	{ name: "network allow", permissions: { network: "allow" as const } },
])("Python HTTP cannot reach an unowned host loopback port under $name", async ({
	permissions,
}) => {
	let hostRequests = 0;
	const server = createServer((_request, response) => {
		hostRequests += 1;
		response.end("host-only-secret");
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const port = (server.address() as AddressInfo).port;
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({ permissions });
		const result = await vm.execArgv("python", [
			"-c",
			`
import urllib.request
try:
    print(urllib.request.urlopen("http://127.0.0.1:${port}/private").read().decode())
except Exception as error:
    print(type(error).__name__ + ": " + str(error))
`,
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(hostRequests, result.stdout).toBe(0);
		expect(result.stdout).toMatch(/ECONNREFUSED|EACCES|loopbackExemptPorts/);
	} finally {
		await vm?.dispose();
		await new Promise<void>((resolve, reject) =>
			server.close((error) => (error ? reject(error) : resolve())),
		);
	}
}, 120_000);

for (const { name, permissions } of [
	{ name: "default policy", permissions: {} },
	{ name: "network deny", permissions: { network: "deny" as const } },
]) {
	for (const host of ["127.0.0.1", "127.0.0.2", "[::1]", "localhost"]) {
		test(`Python HTTP reaches a VM-local binary HTTP server under ${name} at ${host}`, async () => {
			const vm = await AgentOs.create({ permissions });
			let serverPid: string | undefined;
			try {
				await vm.writeFile(
					"/app/http-server.mjs",
					`
import http from 'node:http';
const server = http.createServer((request, response) => {
  const chunks = [];
  request.on('data', chunk => chunks.push(chunk));
  request.on('end', () => {
    response.writeHead(201, { 'x-duplicate': ['one', 'two'], 'set-cookie': ['first=one; Expires=Wed, 01 Jan 2031 00:00:00 GMT', 'second=two'], 'constructor': 'exact-field', 'x-host': request.headers.host, 'x-path': request.url });
    response.end(Buffer.concat(chunks));
  });
});
server.listen(3000, ${JSON.stringify(host === "[::1]" ? "::1" : "0.0.0.0")}, () => console.log('READY'));
`,
				);
				let ready!: () => void;
				const listening = new Promise<void>((resolve) => {
					ready = resolve;
				});
				const server = await vm.process.spawn(
					"node",
					["/app/http-server.mjs"],
					{
						onStdout: (chunk) => {
							if (new TextDecoder().decode(chunk).includes("READY")) ready();
						},
					},
				);
				serverPid = server.pid;
				let readyTimer: ReturnType<typeof setTimeout> | undefined;
				try {
					await Promise.race([
						listening,
						new Promise((_, reject) => {
							readyTimer = setTimeout(
								() => reject(new Error("guest HTTP server did not start")),
								10_000,
							);
						}),
					]);
				} finally {
					clearTimeout(readyTimer);
				}
				const result = await vm.execArgv("python", [
					"-c",
					`
import urllib.request, json
request = urllib.request.Request("http://${host}:3000/binary?q=one", data=bytes([0,255,128,10,13,90]), method="POST")
with urllib.request.urlopen(request) as response:
    print(json.dumps({"status": response.status, "body": response.read().hex(), "headers": response.headers.get_all("x-duplicate"), "cookies": response.headers.get_all("set-cookie"), "constructor": response.headers["constructor"], "host": response.headers["x-host"], "path": response.headers["x-path"]}))
`,
				]);
				expect(result.exitCode, result.stderr).toBe(0);
				expect(JSON.parse(result.stdout.trim())).toEqual({
					status: 201,
					body: "00ff800a0d5a",
					headers: ["one", "two"],
					cookies: [
						"first=one; Expires=Wed, 01 Jan 2031 00:00:00 GMT",
						"second=two",
					],
					constructor: "exact-field",
					host: `${host}:3000`,
					path: "/binary?q=one",
				});
			} finally {
				if (serverPid) {
					await vm.process.kill(serverPid);
					await vm.process.wait(serverPid);
				}
				await vm.dispose();
			}
		}, 60_000);
	}
}

test("Python HTTP can reach an explicitly exempted host loopback port", async () => {
	let requests = 0;
	const server = createServer((_request, response) => {
		requests += 1;
		response.end("explicit-host-access");
	});
	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const port = (server.address() as AddressInfo).port;
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			permissions: { network: "allow" },
			loopbackExemptPorts: [port],
		});
		const result = await vm.execArgv("python", [
			"-c",
			`import urllib.request; print(urllib.request.urlopen("http://127.0.0.1:${port}/").read().decode())`,
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.stdout.trim()).toBe("explicit-host-access");
		expect(requests).toBe(1);
	} finally {
		await vm?.dispose();
		await new Promise<void>((resolve, reject) =>
			server.close((error) => (error ? reject(error) : resolve())),
		);
	}
}, 60_000);

test("Python VM-local HTTPS verifies the guest CA bundle", async () => {
	const directory = mkdtempSync(join(tmpdir(), "agentos-http-tls-"));
	let vm: AgentOs | undefined;
	let serverPid: string | undefined;
	const serverEvents: string[] = [];
	try {
		const keyPath = join(directory, "key.pem");
		const certPath = join(directory, "cert.pem");
		execFileSync(
			"openssl",
			[
				"req",
				"-new",
				"-x509",
				"-newkey",
				"ec",
				"-pkeyopt",
				"ec_paramgen_curve:P-256",
				"-nodes",
				"-keyout",
				keyPath,
				"-out",
				certPath,
				"-days",
				"2",
				"-subj",
				"/CN=localhost",
				"-addext",
				"subjectAltName=DNS:localhost,IP:127.0.0.1",
				"-addext",
				"basicConstraints=critical,CA:FALSE",
			],
			{ stdio: "pipe", timeout: 10_000 },
		);
		const key = readFileSync(keyPath, "utf8");
		const cert = readFileSync(certPath, "utf8");
		vm = await AgentOs.create({ permissions: { network: "deny" } });
		await vm.writeFile(
			"/app/https-server.mjs",
			`
import https from 'node:https';
const server = https.createServer({ key: ${JSON.stringify(key)}, cert: ${JSON.stringify(cert)} }, (_request, response) => response.end('guest-tls-ok'));
server.on('secureConnection', () => console.log('SECURE'));
server.on('connection', () => console.log('CONNECTION'));
server.on('tlsClientError', error => console.log('TLS_ERROR:' + error.message));
server.listen(3000, '127.0.0.1', () => console.log('READY'));
`,
		);
		let ready!: () => void;
		const listening = new Promise<void>((resolve) => {
			ready = resolve;
		});
		const server = await vm.process.spawn("node", ["/app/https-server.mjs"], {
			onStderr: (chunk) => {
				serverEvents.push(new TextDecoder().decode(chunk));
			},
			onStdout: (chunk) => {
				const text = new TextDecoder().decode(chunk);
				serverEvents.push(text);
				if (text.includes("READY")) ready();
			},
		});
		serverPid = server.pid;
		let readyTimer: ReturnType<typeof setTimeout> | undefined;
		try {
			await Promise.race([
				listening,
				new Promise((_, reject) => {
					readyTimer = setTimeout(
						() => reject(new Error("guest HTTPS server did not start")),
						10_000,
					);
				}),
			]);
		} finally {
			clearTimeout(readyTimer);
		}
		const rejected = await vm.execArgv("python", [
			"-c",
			`
import urllib.request
try:
    print(urllib.request.urlopen("https://127.0.0.1:3000/").read().decode())
except Exception as error:
    print(str(error))
`,
		]);
		expect(rejected.exitCode, rejected.stderr).toBe(0);
		expect(rejected.stdout).toMatch(/certificate|issuer/i);
		expect(serverEvents.join("")).not.toContain("SECURE");
		await vm.writeFile("/etc/ssl/certs/ca-certificates.crt", cert);
		const accepted = await vm.execArgv("python", [
			"-c",
			'import urllib.request; print(urllib.request.urlopen("https://127.0.0.1:3000/").read().decode())',
		]);
		expect(accepted.exitCode, accepted.stderr + serverEvents.join("")).toBe(0);
		expect(accepted.stdout.trim()).toBe("guest-tls-ok");
		const named = await vm.execArgv("python", [
			"-c",
			'import urllib.request; print(urllib.request.urlopen("https://localhost:3000/").read().decode())',
		]);
		expect(named.exitCode, named.stderr).toBe(0);
		expect(named.stdout.trim()).toBe("guest-tls-ok");
	} finally {
		if (vm && serverPid) {
			await vm.process.kill(serverPid);
			await vm.process.wait(serverPid);
		}
		await vm?.dispose();
		rmSync(directory, { recursive: true, force: true });
	}
}, 60_000);

test.each([
	"normal",
	"body",
	"aggregate",
])("Python VM-local HTTP preserves response framing and configured limits (%s)", async (mode) => {
	const limited = mode !== "normal";
	const vm = await AgentOs.create({
		permissions: { network: "deny" },
		...(mode === "body"
			? { limits: { http: { maxFetchResponseBytes: 4 } } }
			: mode === "aggregate"
				? {
						limits: {
							http: { maxFetchResponseBytes: 8 * 1024 },
							resources: { maxSocketBufferedBytes: 4 * 1024 * 1024 },
						},
					}
				: {}),
	});
	let pid: string | undefined;
	try {
		await vm.writeFile(
			"/app/response-server.mjs",
			`
import http from 'node:http';
const server = http.createServer((request, response) => {
  if (request.url === '/redirect') { response.writeHead(302, { location: '/final' }); response.end(); }
  else if (request.url === '/error') { response.writeHead(418); response.end('error-body'); }
  else if (request.method === 'HEAD') { response.writeHead(200, { 'content-length': '100' }); response.end(); }
  else { response.write(Buffer.from([0, 255, 128])); response.end(Buffer.from([13, 10, 90])); }
});
server.listen(3000, '127.0.0.1', () => console.log('READY'));
`,
		);
		let ready!: () => void;
		const listening = new Promise<void>((resolve) => {
			ready = resolve;
		});
		const server = await vm.process.spawn(
			"node",
			["/app/response-server.mjs"],
			{
				onStdout: (chunk) => {
					if (new TextDecoder().decode(chunk).includes("READY")) ready();
				},
			},
		);
		pid = server.pid;
		let timer: ReturnType<typeof setTimeout> | undefined;
		try {
			await Promise.race([
				listening,
				new Promise((_, reject) => {
					timer = setTimeout(
						() => reject(new Error("response server did not start")),
						10_000,
					);
				}),
			]);
		} finally {
			clearTimeout(timer);
		}
		const result = await vm.execArgv("python", [
			"-c",
			limited
				? `
import urllib.request
try:
    urllib.request.urlopen("http://127.0.0.1:3000/chunks", data=${mode === "aggregate" ? "bytes(2 * 1024 * 1024)" : "None"}).read()
except Exception as error:
    print(str(error))
`
				: `
import urllib.request, urllib.error, json
result = {}
with urllib.request.urlopen("http://127.0.0.1:3000/redirect") as response:
    result["redirect"] = [response.status, response.headers["location"]]
try:
    urllib.request.urlopen("http://127.0.0.1:3000/error")
except urllib.error.HTTPError as error:
    result["error"] = [error.code, error.read().decode()]
with urllib.request.urlopen(urllib.request.Request("http://127.0.0.1:3000/head", method="HEAD")) as response:
    result["head"] = [response.status, response.read().hex()]
with urllib.request.urlopen("http://127.0.0.1:3000/chunks") as response:
    result["chunks"] = response.read().hex()
print(json.dumps(result))
`,
		]);
		expect(result.exitCode, result.stderr).toBe(0);
		if (limited)
			expect(result.stdout).toMatch(
				mode === "body" ? /maxFetchResponseBytes/ : /maxSocketBufferedBytes/,
			);
		else
			expect(JSON.parse(result.stdout.trim())).toEqual({
				redirect: [302, "/final"],
				error: [418, "error-body"],
				head: [200, ""],
				chunks: "00ff800d0a5a",
			});
	} finally {
		if (pid) {
			await vm.process.kill(pid);
			await vm.process.wait(pid);
		}
		await vm.dispose();
	}
}, 60_000);
