import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { execSync } from "node:child_process";
import {
	existsSync,
	mkdtempSync,
	readFileSync,
	rmSync,
	unlinkSync,
	writeFileSync,
} from "node:fs";
import {
	createServer,
	type IncomingMessage,
	type Server,
	type ServerResponse,
} from "node:http";
import {
	createServer as createHttpsServer,
	type Server as HttpsServer,
} from "node:https";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { gzipSync } from "node:zlib";
import { createWasmVmRuntime } from "@agentos/test-harness";
import {
	allowAll,
	C_BUILD_DIR,
	COMMANDS_DIR,
	createInMemoryFileSystem,
	createKernel,
	describeIf,
	itIf,
} from "@agentos/test-harness";
import type { Kernel } from "@agentos/test-harness";

const WGET_COMMAND_DIRS = [C_BUILD_DIR, COMMANDS_DIR].filter((dir) =>
	existsSync(dir),
);
const hasWgetBinary = WGET_COMMAND_DIRS.some((dir) =>
	existsSync(resolve(dir, "wget")),
);
const WGET_EXEC_TIMEOUT_MS = 10_000;

let hasOpenssl = false;
try {
	execSync("openssl version", { stdio: "pipe" });
	hasOpenssl = true;
} catch {
	hasOpenssl = false;
}

// A long, highly compressible payload so the gzipped body is clearly distinct
// from the plaintext (proving wget actually inflated it via zlib).
const COMPRESSION_PAYLOAD =
	"agentos-wget-compression " +
	"the quick brown fox jumps over the lazy dog. ".repeat(64);

// Build a real CA and a leaf cert signed by it, with a SAN covering the
// 127.0.0.1 loopback endpoint the tests connect to. This lets wget's mbedTLS
// backend perform genuine chain + hostname verification, exactly like Linux
// wget against a private CA.
function makeCaSignedCert(caCommonName: string): {
	caPem: string;
	serverKey: string;
	serverCert: string;
} {
	const dir = mkdtempSync(join(tmpdir(), "wget-ca-"));
	try {
		execSync(
			`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "${dir}/ca.key" 2>/dev/null`,
		);
		execSync(
			`openssl req -x509 -new -key "${dir}/ca.key" -days 3650 -subj "/CN=${caCommonName}" -out "${dir}/ca.crt" 2>/dev/null`,
		);
		execSync(
			`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "${dir}/srv.key" 2>/dev/null`,
		);
		execSync(
			`openssl req -new -key "${dir}/srv.key" -subj "/CN=localhost" -out "${dir}/srv.csr" 2>/dev/null`,
		);
		writeFileSync(`${dir}/ext.cnf`, "subjectAltName=DNS:localhost,IP:127.0.0.1\n");
		execSync(
			`openssl x509 -req -in "${dir}/srv.csr" -CA "${dir}/ca.crt" -CAkey "${dir}/ca.key" ` +
				`-CAcreateserial -days 3650 -extfile "${dir}/ext.cnf" -out "${dir}/srv.crt" 2>/dev/null`,
		);
		return {
			caPem: readFileSync(`${dir}/ca.crt`, "utf8"),
			serverKey: readFileSync(`${dir}/srv.key`, "utf8"),
			serverCert: readFileSync(`${dir}/srv.crt`, "utf8"),
		};
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

function generateSelfSignedCert(): { key: string; cert: string } {
	const keyPath = join(tmpdir(), `wget-test-key-${process.pid}-${Date.now()}.pem`);
	try {
		const key = execSync(
			"openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 2>/dev/null",
			{ encoding: "utf8" },
		);
		writeFileSync(keyPath, key);
		const cert = execSync(
			`openssl req -new -x509 -key "${keyPath}" -days 1 -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" 2>/dev/null`,
			{ encoding: "utf8" },
		);
		return { key, cert };
	} finally {
		try {
			unlinkSync(keyPath);
		} catch {
			// Best effort cleanup for test temp files.
		}
	}
}

describeIf(hasWgetBinary, "wget command", () => {
	let kernel: Kernel;
	let server: Server;
	let selfSignedServer: HttpsServer;
	let validHttpsServer: HttpsServer;
	let caHttpsServer: HttpsServer;
	let port: number;
	let selfSignedPort: number;
	let validHttpsPort: number;
	let caHttpsPort: number;
	// CA (PEM) trusted by the seeded /etc/ssl/certs/ca-certificates.crt bundle;
	// it signs validHttpsServer's leaf. caOnlyPem signs caHttpsServer's leaf and
	// is deliberately NOT in the bundle, so it verifies only via
	// --ca-certificate.
	let seededCaPem = "";
	let caOnlyPem = "";

	beforeAll(async () => {
		server = createServer((req: IncomingMessage, res: ServerResponse) => {
			const url = req.url ?? "/";

			if (url === "/file.txt") {
				res.writeHead(200, { "Content-Type": "text/plain" });
				res.end("downloaded content");
				return;
			}

			if (url === "/data.json") {
				res.writeHead(200, { "Content-Type": "application/json" });
				res.end(JSON.stringify({ status: "ok" }));
				return;
			}

			if (url === "/gzip") {
				const body = gzipSync(Buffer.from(COMPRESSION_PAYLOAD));
				res.writeHead(200, {
					"Content-Type": "text/plain",
					"Content-Encoding": "gzip",
					"Content-Length": String(body.length),
				});
				res.end(body);
				return;
			}

			if (url === "/redirect") {
				res.writeHead(302, {
					Location: `http://127.0.0.1:${port}/redirected`,
				});
				res.end();
				return;
			}

			if (url === "/redirected") {
				res.writeHead(200, { "Content-Type": "text/plain" });
				res.end("arrived after redirect");
				return;
			}

			res.writeHead(404, { "Content-Type": "text/plain" });
			res.end("not found");
		});

		await new Promise<void>((resolveListen) =>
			server.listen(0, "127.0.0.1", resolveListen),
		);
		port = (server.address() as import("node:net").AddressInfo).port;

		if (hasOpenssl) {
			// Self-signed leaf: no chain to any trusted CA -> must fail verify.
			const selfSigned = generateSelfSignedCert();
			selfSignedServer = createHttpsServer(
				{ key: selfSigned.key, cert: selfSigned.cert },
				(req, res) => {
					res.writeHead(200, { "Content-Type": "text/plain" });
					res.end("self-signed secure content");
				},
			);
			await new Promise<void>((resolveListen) =>
				selfSignedServer.listen(0, "127.0.0.1", resolveListen),
			);
			selfSignedPort = (
				selfSignedServer.address() as import("node:net").AddressInfo
			).port;

			// Leaf chaining to a CA seeded into the guest's bundle -> verifies
			// with no --no-check-certificate / --ca-certificate.
			const trusted = makeCaSignedCert("AgentOS Wget Test Root CA");
			seededCaPem = trusted.caPem;
			validHttpsServer = createHttpsServer(
				{ key: trusted.serverKey, cert: trusted.serverCert },
				(req, res) => {
					res.writeHead(200, { "Content-Type": "text/plain" });
					res.end("verified https content");
				},
			);
			await new Promise<void>((resolveListen) =>
				validHttpsServer.listen(0, "127.0.0.1", resolveListen),
			);
			validHttpsPort = (
				validHttpsServer.address() as import("node:net").AddressInfo
			).port;

			// Leaf whose CA is provided ONLY via --ca-certificate (not in bundle).
			const caOnly = makeCaSignedCert("AgentOS Wget Cacert-Only CA");
			caOnlyPem = caOnly.caPem;
			caHttpsServer = createHttpsServer(
				{ key: caOnly.serverKey, cert: caOnly.serverCert },
				(req, res) => {
					res.writeHead(200, { "Content-Type": "text/plain" });
					res.end("cacert https content");
				},
			);
			await new Promise<void>((resolveListen) =>
				caHttpsServer.listen(0, "127.0.0.1", resolveListen),
			);
			caHttpsPort = (
				caHttpsServer.address() as import("node:net").AddressInfo
			).port;
		}
	});

	afterAll(async () => {
		for (const s of [
			server,
			selfSignedServer,
			validHttpsServer,
			caHttpsServer,
		]) {
			if (s) {
				await new Promise<void>((resolveClose) => s.close(() => resolveClose()));
			}
		}
	});

	afterEach(async () => {
		await kernel?.dispose();
	});

	async function mountKernel() {
		const filesystem = createInMemoryFileSystem();
		kernel = createKernel({
			filesystem,
			permissions: allowAll,
			loopbackExemptPorts: [
				port,
				selfSignedPort,
				validHttpsPort,
				caHttpsPort,
			].filter((p) => typeof p === "number"),
		});
		await kernel.mount(createWasmVmRuntime({ commandDirs: WGET_COMMAND_DIRS }));

		// Seed the Debian-shaped trust store the way the native VM bootstrap
		// does, so wget's default CA bundle resolves in-guest. Only the
		// "trusted" CA is placed here; the cacert-only CA is intentionally
		// absent.
		if (seededCaPem) {
			await filesystem.mkdir("/etc/ssl/certs", { recursive: true });
			await kernel.writeFile("/etc/ssl/certs/ca-certificates.crt", seededCaPem);
		}
		return filesystem;
	}

	it("downloads a file using the URL basename", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(`wget http://127.0.0.1:${port}/file.txt`, {
			timeout: WGET_EXEC_TIMEOUT_MS,
		});

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/workspace/file.txt")).toBe(
			"downloaded content",
		);
	}, 15_000);

	it("-O saves to the requested output path", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -O /output.txt http://127.0.0.1:${port}/data.json`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/output.txt")).toContain(
			'"status":"ok"',
		);
	}, 15_000);

	it("-q suppresses progress output", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -q -O /quiet.txt http://127.0.0.1:${port}/file.txt`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stderr).toBe("");
		expect(await filesystem.readTextFile("/quiet.txt")).toBe(
			"downloaded content",
		);
	}, 15_000);

	it("reports failure for a 404 URL", async () => {
		await mountKernel();

		const result = await kernel.exec(
			`wget http://127.0.0.1:${port}/missing.txt`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode).not.toBe(0);
		expect(result.stderr).toMatch(/404|not found|error/i);
	}, 15_000);

	it("follows redirects by default", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget -O /redirected.txt http://127.0.0.1:${port}/redirect`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/redirected.txt")).toBe(
			"arrived after redirect",
		);
	}, 15_000);

	it("--version reports the mbedTLS HTTPS backend", async () => {
		await mountKernel();

		const result = await kernel.exec("wget --version", {
			timeout: WGET_EXEC_TIMEOUT_MS,
		});

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(result.stdout).toContain("GNU Wget 1.24.5");
		// Real in-guest TLS: HTTPS is compiled in and the backend is mbedTLS.
		expect(result.stdout).toMatch(/\+https/);
		expect(result.stdout).toMatch(/ssl\/mbedtls/i);
	}, 15_000);

	it("--compression=auto inflates a gzip response body", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget --compression=auto -O /gz.txt http://127.0.0.1:${port}/gzip`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/gz.txt")).toBe(COMPRESSION_PAYLOAD);
	}, 15_000);

	itIf(hasOpenssl, "downloads over HTTPS verifying against the seeded CA bundle", async () => {
		const filesystem = await mountKernel();

		// No --no-check-certificate, no --ca-certificate: trust comes solely
		// from the seeded /etc/ssl/certs/ca-certificates.crt, like Debian wget.
		const result = await kernel.exec(
			`wget -O /secure.txt https://127.0.0.1:${validHttpsPort}/file`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/secure.txt")).toBe(
			"verified https content",
		);
	}, 15_000);

	itIf(hasOpenssl, "fails with a real cert error on an untrusted (self-signed) server", async () => {
		await mountKernel();

		const result = await kernel.exec(
			`wget -O /nope.txt https://127.0.0.1:${selfSignedPort}/file`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		// VERIFCERTERR -> WGET_EXIT_SSL_AUTH_FAIL == 5, the native taxonomy.
		expect(result.exitCode).toBe(5);
		expect(result.stderr).toMatch(/cannot verify|certificate|not trusted/i);
	}, 15_000);

	itIf(hasOpenssl, "--no-check-certificate accepts a self-signed server", async () => {
		const filesystem = await mountKernel();

		const result = await kernel.exec(
			`wget --no-check-certificate -O /insecure.txt https://127.0.0.1:${selfSignedPort}/file`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/insecure.txt")).toBe(
			"self-signed secure content",
		);
	}, 15_000);

	itIf(hasOpenssl, "--ca-certificate trusts a server signed by that CA", async () => {
		const filesystem = await mountKernel();

		// caHttpsServer's CA is NOT in the seeded bundle, so this only passes if
		// --ca-certificate is honored (real file read + chain build in-guest).
		await kernel.writeFile("/tmp/cacert-only.pem", caOnlyPem);
		const result = await kernel.exec(
			`wget --ca-certificate=/tmp/cacert-only.pem -O /cacert.txt https://127.0.0.1:${caHttpsPort}/file`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode, result.stderr || result.stdout).toBe(0);
		expect(await filesystem.readTextFile("/cacert.txt")).toBe(
			"cacert https content",
		);
	}, 15_000);

	itIf(hasOpenssl, "--ca-certificate with the wrong CA still fails verification", async () => {
		await mountKernel();

		// Point --ca-certificate at the seeded CA, which did NOT sign
		// caHttpsServer's leaf.
		await kernel.writeFile("/tmp/wrong-ca.pem", seededCaPem);
		const result = await kernel.exec(
			`wget --ca-certificate=/tmp/wrong-ca.pem -O /wrong.txt https://127.0.0.1:${caHttpsPort}/file`,
			{ timeout: WGET_EXEC_TIMEOUT_MS },
		);

		expect(result.exitCode).toBe(5);
		expect(result.stderr).toMatch(/cannot verify|certificate|not trusted/i);
	}, 15_000);
});
