import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:https";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs, type Permissions } from "../src/index.js";

const CHILD_SOURCE = `
const https = require('node:https');
const port = Number(process.argv[1]);
const trust = process.argv[2];
const certificate = process.argv[3];
const options = { host: '127.0.0.1', port, path: '/probe', agent: false };
if (trust === 'ca') options.ca = certificate;
if (trust === 'insecure') options.rejectUnauthorized = false;
const request = https.get(options, (response) => {
  let body = '';
  response.setEncoding('utf8');
  response.on('data', (chunk) => { body += chunk; });
  response.on('end', () => console.log(JSON.stringify({ status: response.statusCode, body })));
});
request.on('error', (error) => console.log(JSON.stringify({ error: error.code ?? error.cause?.code ?? error.name })));
request.setTimeout(4000, () => request.destroy(new Error('fixture timeout')));
`;

function parentSource(
	port: number,
	trust: "ca" | "default" | "insecure",
	certificate: string,
): string {
	return `
const { spawn } = require('node:child_process');
const marker = require('node:fs').readFileSync('/tmp/network-probe.txt', 'utf8');
const child = spawn('node', ['-e', ${JSON.stringify(CHILD_SOURCE)}, ${JSON.stringify(String(port))},
  ${JSON.stringify(trust)}, ${JSON.stringify(certificate)}],
  { stdio: ['ignore', 'pipe', 'pipe'] });
let stdout = '';
let stderr = '';
child.stdout.setEncoding('utf8');
child.stderr.setEncoding('utf8');
child.stdout.on('data', (chunk) => { stdout += chunk; });
child.stderr.on('data', (chunk) => { stderr += chunk; });
child.on('error', (error) => { console.error(error.stack); process.exitCode = 1; });
child.on('close', (code) => console.log(JSON.stringify({ code, stdout, stderr, marker })));
`;
}

function makeCertificate(): {
	key: string;
	cert: string;
	ca: string;
	dispose: () => void;
} {
	const directory = mkdtempSync(join(tmpdir(), "agentos-network-rule-tls-"));
	const keyPath = join(directory, "key.pem");
	const certPath = join(directory, "cert.pem");
	const caKeyPath = join(directory, "ca-key.pem");
	const caPath = join(directory, "ca.pem");
	const csrPath = join(directory, "leaf.csr");
	const extensionsPath = join(directory, "leaf.ext");
	writeFileSync(
		extensionsPath,
		"basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
	);
	const commands = [
		[
			"req",
			"-x509",
			"-newkey",
			"ec",
			"-pkeyopt",
			"ec_paramgen_curve:P-256",
			"-nodes",
			"-keyout",
			caKeyPath,
			"-out",
			caPath,
			"-days",
			"2",
			"-subj",
			"/CN=agentos-test-ca",
			"-addext",
			"basicConstraints=critical,CA:TRUE",
			"-addext",
			"keyUsage=critical,keyCertSign,cRLSign",
		],
		[
			"req",
			"-new",
			"-newkey",
			"ec",
			"-pkeyopt",
			"ec_paramgen_curve:P-256",
			"-nodes",
			"-keyout",
			keyPath,
			"-out",
			csrPath,
			"-subj",
			"/CN=localhost",
		],
		[
			"x509",
			"-req",
			"-in",
			csrPath,
			"-CA",
			caPath,
			"-CAkey",
			caKeyPath,
			"-CAcreateserial",
			"-out",
			certPath,
			"-days",
			"2",
			"-extfile",
			extensionsPath,
		],
	];
	for (const args of commands) {
		const result = spawnSync("openssl", args, {
			encoding: "utf8",
			timeout: 10_000,
		});
		if (result.status !== 0) {
			rmSync(directory, { recursive: true, force: true });
			throw new Error(`local certificate generation failed: ${result.stderr}`);
		}
	}
	return {
		key: readFileSync(keyPath, "utf8"),
		cert: readFileSync(certPath, "utf8"),
		ca: readFileSync(caPath, "utf8"),
		dispose: () => rmSync(directory, { recursive: true, force: true }),
	};
}

describe("network rules apply to nested Node HTTPS processes", () => {
	let certificate: ReturnType<typeof makeCertificate>;
	let allowedServer: Server;
	let adjacentServer: Server;
	let allowedPort: number;
	let adjacentPort: number;
	let allowedRequests = 0;
	let adjacentRequests = 0;
	async function closeServer(server?: Server) {
		if (!server?.listening) return;
		await new Promise<void>((resolve, reject) =>
			server.close((error) => (error ? reject(error) : resolve())),
		);
	}

	beforeAll(async () => {
		try {
			certificate = makeCertificate();
			allowedServer = createServer(certificate, (_request, response) => {
				allowedRequests++;
				response.end("allowed-response");
			});
			adjacentServer = createServer(certificate, (_request, response) => {
				adjacentRequests++;
				response.end("adjacent-response");
			});
			await Promise.all(
				[allowedServer, adjacentServer].map(
					(server) =>
						new Promise<void>((resolve, reject) => {
							server.once("error", reject);
							server.listen(0, "127.0.0.1", resolve);
						}),
				),
			);
			allowedPort = (allowedServer.address() as AddressInfo).port;
			adjacentPort = (adjacentServer.address() as AddressInfo).port;
		} catch (error) {
			await Promise.all([
				closeServer(allowedServer),
				closeServer(adjacentServer),
			]);
			certificate?.dispose();
			throw error;
		}
	});

	afterAll(async () => {
		try {
			await Promise.all([
				closeServer(allowedServer),
				closeServer(adjacentServer),
			]);
		} finally {
			certificate?.dispose();
		}
	});

	async function probe(
		permissions: Permissions,
		port: number,
		exemptHostPort = true,
		trust: "ca" | "default" | "insecure" = "insecure",
	) {
		const vm = await AgentOs.create({
			defaultSoftware: false,
			permissions,
			loopbackExemptPorts: exemptHostPort ? [allowedPort, adjacentPort] : [],
		});
		try {
			await vm.writeFile("/tmp/network-probe.txt", "merged-fs-ok");
			const result = await vm.process.execFile(
				"node",
				["-e", parentSource(port, trust, certificate.ca)],
				{
					timeoutMs: 10_000,
					output: { capture: "all" },
				},
			);
			expect(result.exitCode, result.stderr).toBe(0);
			const parent = JSON.parse(
				result.stdout.trim().split("\n").at(-1) ?? "",
			) as {
				code: number;
				stdout: string;
				stderr: string;
				marker: string;
			};
			expect(parent.code, parent.stderr).toBe(0);
			expect(parent.marker).toBe("merged-fs-ok");
			return JSON.parse(parent.stdout.trim().split("\n").at(-1) ?? "") as {
				status?: number;
				body?: string;
				error?: string;
			};
		} finally {
			await vm.dispose();
		}
	}

	test("an exact endpoint rule grants nested HTTPS while omitted scopes keep defaults", async () => {
		const permissions: Permissions = {
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["http"],
						patterns: [`tcp://127.0.0.1:${allowedPort}`],
					},
				],
			},
		};
		const before = allowedRequests;
		expect(await probe(permissions, allowedPort)).toEqual({
			status: 200,
			body: "allowed-response",
		});
		expect(allowedRequests).toBe(before + 1);
		const adjacentBefore = adjacentRequests;
		expect((await probe(permissions, adjacentPort)).error).toBe("EACCES");
		expect(adjacentRequests).toBe(adjacentBefore);
	}, 60_000);

	test("a bare host rule does not match an endpoint; host loopback needs an exemption", async () => {
		const bareHost: Permissions = {
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["http"],
						patterns: ["127.0.0.1"],
					},
				],
			},
		};
		const before = allowedRequests;
		expect((await probe(bareHost, allowedPort)).error).toBe("EACCES");
		expect((await probe({}, allowedPort, false)).error).toMatch(
			/EACCES|EPERM|ECONNREFUSED/,
		);
		expect(allowedRequests).toBe(before);
		expect(await probe({}, allowedPort)).toEqual({
			status: 200,
			body: "allowed-response",
		});
		expect(allowedRequests).toBe(before + 1);
	}, 60_000);

	test("nested HTTPS rejects an untrusted certificate and accepts the configured CA", async () => {
		const permissions: Permissions = {
			network: {
				default: "deny",
				rules: [
					{
						mode: "allow",
						operations: ["http"],
						patterns: [`tcp://127.0.0.1:${allowedPort}`],
					},
				],
			},
		};
		const before = allowedRequests;
		expect(
			(await probe(permissions, allowedPort, true, "default")).error,
		).toMatch(
			/^(DEPTH_ZERO_SELF_SIGNED_CERT|SELF_SIGNED_CERT_IN_CHAIN|UNABLE_TO_VERIFY_LEAF_SIGNATURE)$/,
		);
		expect(allowedRequests).toBe(before);
		expect(await probe(permissions, allowedPort, true, "ca")).toEqual({
			status: 200,
			body: "allowed-response",
		});
		expect(allowedRequests).toBe(before + 1);
	}, 60_000);
});
