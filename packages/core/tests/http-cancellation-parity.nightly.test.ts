import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const HTTP_HEADERS_ABORT = `
const http = require('node:http');
const events = [];
let pending;
const server = http.createServer((request, response) => {
  if (request.url === '/stall') {
    setImmediate(() => pending.destroy(new Error('cancel-headers')));
    return;
  }
  response.end('retry-ok');
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = 'http://127.0.0.1:' + server.address().port;
  await new Promise((resolve) => {
    pending = http.get(base + '/stall', { agent: false }, () => events.push('response'));
    pending.on('error', (error) => events.push('error:' + error.message));
    pending.on('close', () => { events.push('close'); resolve(); });
  });
  const retry = await new Promise((resolve, reject) => {
    http.get(base + '/retry', { agent: false }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events, retry }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;

const HTTP_BODY_ABORT = `
const http = require('node:http');
const events = [];
const server = http.createServer((request, response) => {
  if (request.url === '/stream') {
    response.writeHead(200, { 'content-type': 'text/plain' });
    response.write('first-chunk');
    return;
  }
  response.end('retry-ok');
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const base = 'http://127.0.0.1:' + server.address().port;
  await new Promise((resolve, reject) => {
    let responseReceived = false;
    const request = http.get(base + '/stream', { agent: false }, (response) => {
      responseReceived = true;
      response.setEncoding('utf8');
      response.on('data', (chunk) => {
        events.push('data:' + chunk);
        response.destroy(new Error('cancel-body'));
      });
      response.on('error', (error) => events.push('error:' + error.message));
      response.on('close', () => { events.push('close'); resolve(); });
    });
    request.on('error', (error) => { if (!responseReceived) reject(error); });
  });
  const retry = await new Promise((resolve, reject) => {
    http.get(base + '/retry', { agent: false }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events, retry }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;

function tlsHandshakeRecovery(
	key: string,
	cert: string,
	ca: string,
	rejectOnly = false,
): string {
	return `
const https = require('node:https');
const events = [];
const socketEvents = [];
let secureConnections = 0;
const server = https.createServer({ key: ${JSON.stringify(key)}, cert: ${JSON.stringify(cert)} },
  (_request, response) => response.end('tls-retry-ok'));
server.on('secureConnection', () => { secureConnections++; });
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const options = {
    host: '127.0.0.1', port: server.address().port,
    servername: 'localhost', agent: false,
  };
  await new Promise((resolve) => {
    const request = https.get(options, () => events.push('response'));
    request.on('socket', (socket) => {
      socket.on('error', () => socketEvents.push('socket:error'));
      socket.on('close', () => socketEvents.push('socket:close'));
    });
    request.on('error', () => events.push('error'));
    request.on('close', () => { events.push('close'); resolve(); });
  });
  if (${rejectOnly}) {
    await new Promise((resolve) => server.close(resolve));
    clearTimeout(watchdog);
    console.log(JSON.stringify({ events, socketEvents, secureConnections }));
    return;
  }
  const retry = await new Promise((resolve, reject) => {
    https.get({ ...options, ca: ${JSON.stringify(ca)} }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('end', () => resolve({ status: response.statusCode, body }));
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events, socketEvents, secureConnections, retry }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

function tlsConnectionOrder(key: string, cert: string): string {
	return `
const https = require('node:https');
const events = [];
const server = https.createServer({ key: ${JSON.stringify(key)}, cert: ${JSON.stringify(cert)} },
  (_request, response) => response.end('ok'));
server.on('connection', () => events.push('connection'));
server.on('secureConnection', () => events.push('secureConnection'));
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const body = await new Promise((resolve, reject) => {
    https.get({ host: '127.0.0.1', port: server.address().port,
      rejectUnauthorized: false, agent: false }, (response) => {
      let value = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { value += chunk; });
      response.on('end', () => resolve(value));
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events, body }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

function rawTlsRejection(key: string, cert: string): string {
	return `
const tls = require('node:tls');
const events = [];
const server = tls.createServer({ key: ${JSON.stringify(key)}, cert: ${JSON.stringify(cert)} });
const watchdog = setTimeout(() => process.exit(124), 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  await new Promise((resolve) => {
    const socket = tls.connect({ host: '127.0.0.1', port: server.address().port,
      servername: 'localhost' });
    socket.on('secureConnect', () => events.push('secureConnect'));
    socket.on('error', () => events.push('error'));
    socket.on('close', () => { events.push('close'); resolve(); });
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

function delayedBodyDrain(framing: "length" | "close"): string {
	return `
const http = require('node:http');
const net = require('node:net');
const { Readable } = require('node:stream');
const expected = Buffer.from('0123456789abcdef'.repeat(4096));
const server = net.createServer((socket) => {
  socket.once('data', () => {
    const headers = 'HTTP/1.1 200 OK\\r\\n' +
      ${framing === "length" ? "'Content-Length: ' + expected.length + '\\r\\n' +" : "'' +"}
      'Connection: close\\r\\n\\r\\n';
    socket.end(Buffer.concat([Buffer.from(headers), expected]));
  });
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const result = await new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: server.address().port, agent: false }, async (response) => {
      try {
        const identity = response instanceof Readable;
        response.pause();
        await new Promise((resume) => setTimeout(resume, 20));
        const buffered = response.readableLength;
        const highWaterMark = response.readableHighWaterMark;
        const chunks = [];
        for await (const chunk of response) chunks.push(Buffer.from(chunk));
        const body = Buffer.concat(chunks);
        resolve({ identity, buffered: buffered > 0 && buffered <= 131072,
          highWaterMark: highWaterMark > 0, exact: body.equals(expected),
          bytes: body.length, complete: response.complete, readableEnded: response.readableEnded });
      } catch (error) { reject(error); }
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify(result));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

const SPLIT_CHUNK_AND_HEADERS = `
const http = require('node:http');
const net = require('node:net');
const { Writable } = require('node:stream');
const { pipeline } = require('node:stream/promises');
const server = net.createServer((socket) => {
  socket.once('data', async () => {
    const parts = [
      'HTTP/1.1 200 OK\\r\\nTransfer-Encoding: chunked\\r\\n' +
        'constructor: ctor\\r\\ntoString: text\\r\\n__proto__: proto\\r\\n\\r\\n',
      '5;ext=1\\r', '\\nhello\\r', '\\n', '0\\r\\n',
      'x-tail: value\\r', '\\n', '\\r\\n',
    ];
    for (const part of parts) {
      socket.write(part);
      await new Promise((resolve) => setImmediate(resolve));
    }
    socket.end();
  });
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const result = await new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port: server.address().port, agent: false }, async (response) => {
      try {
        const chunks = [];
        await pipeline(response, new Writable({
          write(chunk, _encoding, done) { chunks.push(Buffer.from(chunk)); done(); },
        }));
        resolve({ body: Buffer.concat(chunks).toString(),
          constructor: response.headers.constructor,
          tostring: response.headers.tostring,
          protoOwn: Object.hasOwn(response.headers, '__proto__'),
          rawProto: response.rawHeaders.includes('__proto__'),
          trailer: response.trailers['x-tail'],
          complete: response.complete });
      } catch (error) { reject(error); }
    }).on('error', reject);
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify(result));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;

const RESPONSE_TIMEOUT = `
const http = require('node:http');
const server = http.createServer((_request, response) => {
  response.writeHead(200, { 'content-type': 'text/plain' });
  response.write('first');
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const events = [];
  await new Promise((resolve, reject) => {
    const request = http.get({ host: '127.0.0.1', port: server.address().port, agent: false }, (response) => {
      response.on('data', () => {});
      response.setTimeout(20, () => { events.push('timeout'); response.destroy(new Error('stop')); });
      response.on('error', (error) => events.push('error:' + error.message));
      response.on('close', () => { events.push('close'); resolve(); });
    });
    request.on('error', (error) => { if (!events.includes('timeout')) reject(error); });
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;

function malformedResponse(frame: string): string {
	return `
const http = require('node:http');
const net = require('node:net');
const events = [];
const server = net.createServer((socket) => socket.once('data', () => socket.end(${JSON.stringify(frame)})));
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  await new Promise((resolve) => {
    const request = http.get({ host: '127.0.0.1', port: server.address().port, agent: false }, (response) => {
      events.push('response');
      response.on('error', () => events.push('response-error'));
      response.on('close', () => events.push('response-close'));
      response.on('end', () => events.push('response-end'));
      response.resume();
    });
    request.on('error', () => events.push('request-error'));
    request.on('close', () => { events.push('request-close'); setImmediate(resolve); });
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

const KEEP_ALIVE_REUSE = `
const http = require('node:http');
let connections = 0;
const server = http.createServer((_request, response) => response.end('ok'));
server.on('connection', () => { connections++; });
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
  const bodies = [];
  for (let index = 0; index < 2; index++) {
    bodies.push(await new Promise((resolve, reject) => {
      http.get({ host: '127.0.0.1', port: server.address().port, agent }, (response) => {
        let body = '';
        response.setEncoding('utf8');
        response.on('data', (chunk) => { body += chunk; });
        response.on('end', () => resolve(body));
      }).on('error', reject);
    }));
  }
  agent.destroy();
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ bodies, connections }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;

function chunkExtensionLine(split: boolean, extensionBytes: number): string {
	return `
const http = require('node:http');
const net = require('node:net');
const events = [];
const server = net.createServer((socket) => {
  socket.on('error', () => {});
  socket.once('data', () => {
  socket.write('HTTP/1.1 200 OK\\r\\nTransfer-Encoding: chunked\\r\\n\\r\\n');
  const line = '1;' + 'a'.repeat(${extensionBytes}) + '\\r\\n';
  if (${split}) {
    socket.write(line.slice(0, 600));
    setImmediate(() => socket.end(line.slice(600) + 'x\\r\\n0\\r\\n\\r\\n'));
  } else {
    socket.end(line + 'x\\r\\n0\\r\\n\\r\\n');
  }
  });
});
const watchdog = setTimeout(() => { console.error('fixture timeout'); process.exit(124); }, 5000);
(async () => {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  await new Promise((resolve) => {
    const request = http.get({ host: '127.0.0.1', port: server.address().port, agent: false }, (response) => {
      events.push('response');
      response.on('data', () => {});
      response.on('error', () => events.push('response-error'));
      response.on('end', () => events.push('response-end'));
      response.on('close', () => { events.push('response-close'); resolve(); });
    });
    request.on('error', () => { events.push('request-error'); resolve(); });
  });
  await new Promise((resolve) => server.close(resolve));
  clearTimeout(watchdog);
  console.log(JSON.stringify({ events }));
})().catch((error) => { console.error(error.stack); process.exitCode = 1; });
`;
}

function makeTlsCertificate(): {
	key: string;
	cert: string;
	ca: string;
	dispose: () => void;
} {
	const directory = mkdtempSync(join(tmpdir(), "agentos-http-cancel-tls-"));
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
			throw new Error(`openssl fixture certificate failed: ${result.stderr}`);
		}
	}
	return {
		key: readFileSync(keyPath, "utf8"),
		cert: readFileSync(certPath, "utf8"),
		ca: readFileSync(caPath, "utf8"),
		dispose: () => rmSync(directory, { recursive: true, force: true }),
	};
}

describe("guest HTTP cancellation and TLS lifecycle match native Node", () => {
	let vm: AgentOs;
	afterEach(async () => {
		await vm?.dispose();
	});

	async function compare(source: string): Promise<Record<string, unknown>> {
		const native = spawnSync(process.execPath, ["-e", source], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.error).toBeUndefined();
		expect(native.status, native.stderr).toBe(0);
		vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 10_000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stderr).toBe("");
		const nativeResult = JSON.parse(native.stdout.trim()) as Record<
			string,
			unknown
		>;
		const guestResult = JSON.parse(guest.stdout.trim()) as Record<
			string,
			unknown
		>;
		expect(guestResult).toEqual(nativeResult);
		return guestResult;
	}

	test("abort before response headers, then reuse the listener", async () => {
		const result = await compare(HTTP_HEADERS_ABORT);
		expect(result).toEqual({
			events: ["error:cancel-headers", "close"],
			retry: { status: 200, body: "retry-ok" },
		});
	}, 30_000);

	test("abort while streaming the response body, then reuse the listener", async () => {
		const result = await compare(HTTP_BODY_ABORT);
		expect(result).toEqual({
			events: ["data:first-chunk", "error:cancel-body", "close"],
			retry: { status: 200, body: "retry-ok" },
		});
	}, 30_000);

	test("certificate rejection closes and a verified HTTPS request succeeds", async () => {
		const fixture = makeTlsCertificate();
		try {
			const result = await compare(
				tlsHandshakeRecovery(fixture.key, fixture.cert, fixture.ca),
			);
			expect(result).toEqual({
				events: ["error", "close"],
				socketEvents: ["socket:error", "socket:close"],
				secureConnections: 1,
				retry: { status: 200, body: "tls-retry-ok" },
			});
		} finally {
			fixture.dispose();
		}
	}, 30_000);

	test("certificate rejection alone closes the HTTPS request", async () => {
		const fixture = makeTlsCertificate();
		try {
			const result = await compare(
				tlsHandshakeRecovery(fixture.key, fixture.cert, fixture.ca, true),
			);
			expect(result).toEqual({
				events: ["error", "close"],
				socketEvents: ["socket:error", "socket:close"],
				secureConnections: 0,
			});
		} finally {
			fixture.dispose();
		}
	}, 30_000);

	test("raw TLS certificate rejection closes the socket", async () => {
		const fixture = makeTlsCertificate();
		try {
			const result = await compare(rawTlsRejection(fixture.key, fixture.cert));
			expect(result).toEqual({ events: ["error", "close"] });
		} finally {
			fixture.dispose();
		}
	}, 30_000);

	test("TLS server connection event precedes secureConnection", async () => {
		const fixture = makeTlsCertificate();
		try {
			const result = await compare(
				tlsConnectionOrder(fixture.key, fixture.cert),
			);
			expect(result).toEqual({
				events: ["connection", "secureConnection"],
				body: "ok",
			});
		} finally {
			fixture.dispose();
		}
	}, 30_000);

	test.each([
		"length",
		"close",
	] as const)("delayed consumer drains exact %s-delimited body through Readable", async (framing) => {
		const result = await compare(delayedBodyDrain(framing));
		expect(result).toEqual({
			identity: true,
			buffered: true,
			highWaterMark: true,
			exact: true,
			bytes: 65536,
			complete: true,
			readableEnded: true,
		});
	}, 30_000);

	test("split chunk frames, trailer, and inherited header names survive pipeline", async () => {
		const result = await compare(SPLIT_CHUNK_AND_HEADERS);
		expect(result).toEqual({
			body: "hello",
			constructor: "ctor",
			tostring: "text",
			protoOwn: false,
			rawProto: true,
			trailer: "value",
			complete: true,
		});
	}, 30_000);

	test("response timeout emits before cancellation error and close", async () => {
		const result = await compare(RESPONSE_TIMEOUT);
		expect(result).toEqual({ events: ["timeout", "error:stop", "close"] });
	}, 30_000);

	test("invalid response head rejects the request before publishing a response", async () => {
		const result = await compare(
			malformedResponse(
				"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n",
			),
		);
		expect(result).toEqual({ events: ["request-error", "request-close"] });
	}, 30_000);

	test("invalid chunk framing errors the published response", async () => {
		const result = await compare(
			malformedResponse(
				"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nZ\r\n",
			),
		);
		expect(result.events).toContain("response");
		expect(result.events).toContain("response-error");
		expect(result.events).not.toContain("response-end");
	}, 30_000);

	test("normal response end preserves a keep-alive socket for the next request", async () => {
		const result = await compare(KEEP_ALIVE_REUSE);
		expect(result).toEqual({ bodies: ["ok", "ok"], connections: 1 });
	}, 30_000);

	test.each([
		false,
		true,
	])("a %s split oversized chunk extension matches Node's 16 KiB bound", async (split) => {
		const source = chunkExtensionLine(split, 16 * 1024 + 1);
		const native = spawnSync(process.execPath, ["-e", source], {
			encoding: "utf8",
			timeout: 10_000,
		});
		expect(native.status, native.stderr).toBe(0);
		const nativeEvents = JSON.parse(native.stdout.trim()).events as string[];
		expect(nativeEvents).toContain("response-error");
		expect(nativeEvents).not.toContain("response-end");
		vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 10_000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		const events = JSON.parse(guest.stdout.trim()).events as string[];
		expect(events).toContain("response");
		expect(events).toContain("response-error");
		expect(events).not.toContain("response-end");
	}, 30_000);

	test("a 16 KiB chunk extension remains accepted", async () => {
		const result = await compare(chunkExtensionLine(false, 16 * 1024));
		expect(result.events).toContain("response-end");
		expect(result.events).not.toContain("response-error");
	}, 30_000);
});
