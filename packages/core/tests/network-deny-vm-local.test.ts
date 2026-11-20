import { createSocket, type Socket } from "node:dgram";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// A scalar `network: "deny"` denies traffic that crosses the VM boundary. Like
// the default policy, it keeps guest listeners and VM-local loopback working,
// matching the loopback behavior of Docker with --network none.

const SERVER_SCRIPT = `
import http from "node:http";
const server = http.createServer((req, res) => res.end("guest-ok:" + req.url));
server.on("error", (error) => {
	console.log("LISTEN_ERROR " + error.code + " " + error.message);
	process.exit(3);
});
server.listen(3000, "127.0.0.1", () => console.log("LISTENING"));
`;

const PROBE_SCRIPT = `
const probe = async (url) => {
	try {
		const response = await fetch(url);
		return { status: response.status, body: await response.text() };
	} catch (error) {
		const cause = error.cause ?? error;
		return { error: String(cause.code ?? cause.name), message: String(cause.message) };
	}
};
const [vmLocal, external, hostLoopback] = await Promise.all([
	probe("http://127.0.0.1:3000/self"),
	probe("http://denied-network.gameinc.io/"),
	probe("http://127.0.0.1:" + process.argv[2] + "/host"),
]);
console.log(JSON.stringify({ vmLocal, external, hostLoopback }));
`;

const LOCAL_SOCKET_SCRIPT = `
import dgram from "node:dgram";
import net from "node:net";

const udp = dgram.createSocket("udp4");
await new Promise((resolve, reject) => {
  udp.once("error", reject);
  udp.bind(0, "127.0.0.1", resolve);
});
const udpBodyPromise = new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("UDP delivery timed out")), 3000);
  udp.once("message", (message) => {
    clearTimeout(timeout);
    resolve(message.toString());
  });
});
await new Promise((resolve, reject) => {
  udp.send(Buffer.from("udp-ok"), udp.address().port, "127.0.0.1", (error) => error ? reject(error) : resolve());
});
const udpBody = await udpBodyPromise;
udp.close();

const path = "/tmp/agentos-vm-local.sock";
const server = net.createServer((socket) => socket.end("unix-ok"));
await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(path, resolve);
});
const unixBody = await new Promise((resolve, reject) => {
  let body = "";
  const client = net.connect(path);
  client.on("data", (chunk) => { body += chunk.toString(); });
  client.once("end", () => resolve(body));
  client.once("error", reject);
});
await new Promise((resolve) => server.close(resolve));
console.log(JSON.stringify({ udpBody, unixBody }));
`;

const RAW_LOOPBACK_PROBE_SCRIPT = `
import net from "node:net";
const socket = net.connect({ host: process.argv[3] ?? "::ffff:127.0.0.1", port: Number(process.argv[2]) });
let response = "";
socket.once("connect", () => {
  socket.write("GET /self HTTP/1.1\\r\\nHost: localhost\\r\\nConnection: close\\r\\n\\r\\n");
});
socket.on("data", (chunk) => { response += chunk.toString(); });
socket.once("end", () => console.log(JSON.stringify({ response })));
socket.once("error", (error) => {
  console.log(JSON.stringify({ error: error.code, message: error.message }));
  process.exitCode = 2;
});
socket.setTimeout(3000, () => socket.destroy(new Error("loopback connect timed out")));
`;

const GUEST_UDP_HOLD_SCRIPT = `
import dgram from "node:dgram";
const socket = dgram.createSocket("udp4");
socket.once("error", (error) => {
  console.log("LISTEN_ERROR " + error.code + " " + error.message);
  process.exit(3);
});
socket.bind(%PORT%, "127.0.0.1", () => console.log("LISTENING"));
`;

const GUEST_UDP_SEND_SCRIPT = `
import dgram from "node:dgram";
const socket = dgram.createSocket("udp4");
try {
  await new Promise((resolve, reject) => {
    socket.send(Buffer.from("guest-udp-probe"), Number(process.argv[2]), "127.0.0.1", (error) => error ? reject(error) : resolve());
  });
  console.log(JSON.stringify({ sent: true }));
} catch (error) {
  console.log(JSON.stringify({ error: error.code ?? error.message }));
} finally {
  socket.close();
}
`;

async function startGuestServer(vm: AgentOs): Promise<number> {
	const decoder = new TextDecoder();
	let stdout = "";
	let listening!: () => void;
	let failed!: (error: Error) => void;
	const ready = new Promise<void>((resolve, reject) => {
		listening = resolve;
		failed = reject;
	});
	const { pid } = await vm.process.spawn("node", ["/app/server.mjs"], {
		onStdout: (chunk) => {
			stdout += decoder.decode(chunk);
			if (stdout.includes("LISTENING")) listening();
			if (stdout.includes("LISTEN_ERROR")) failed(new Error(stdout));
		},
	});
	await ready;
	return pid;
}

async function runProbe(vm: AgentOs, hostPort: number) {
	const decoder = new TextDecoder();
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn(
		"node",
		["/app/probe.mjs", String(hostPort)],
		{
			onStdout: (chunk) => {
				stdout += decoder.decode(chunk);
			},
			onStderr: (chunk) => {
				stderr += decoder.decode(chunk);
			},
		},
	);
	const exit = await vm.process.wait(pid);
	expect(exit.exitCode, stderr).toBe(0);
	return JSON.parse(stdout.trim().split("\n").at(-1) ?? "");
}

async function runRawTcpProbe(vm: AgentOs, port: number, host?: string) {
	const decoder = new TextDecoder();
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn(
		"node",
		["/app/raw-probe.mjs", String(port), ...(host ? [host] : [])],
		{
			onStdout: (chunk) => {
				stdout += decoder.decode(chunk);
			},
			onStderr: (chunk) => {
				stderr += decoder.decode(chunk);
			},
		},
	);
	const exit = await vm.process.wait(pid);
	return {
		exitCode: exit.exitCode,
		stderr,
		...JSON.parse(stdout.trim().split("\n").at(-1) ?? "{}"),
	};
}

describe("VM-local network policy", () => {
	let vm: AgentOs | undefined;
	let hostServer: Server | undefined;
	let hostUdpServer: Socket | undefined;
	let hostRequests = 0;
	let hostDatagrams = 0;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
		await new Promise<void>((resolve) => {
			if (!hostServer) return resolve();
			hostServer.close(() => resolve());
		});
		hostServer = undefined;
		hostRequests = 0;
		await new Promise<void>((resolve) => {
			if (!hostUdpServer) return resolve();
			hostUdpServer.close(() => resolve());
		});
		hostUdpServer = undefined;
		hostDatagrams = 0;
	});

	async function startHostServer(): Promise<number> {
		hostServer = createServer((_req, res) => {
			hostRequests += 1;
			res.end("host-ok");
		});
		await new Promise<void>((resolve) => {
			hostServer?.listen(0, "127.0.0.1", resolve);
		});
		return (hostServer.address() as AddressInfo).port;
	}

	test("keeps VM-local listeners and loopback while denying egress", async () => {
		const hostPort = await startHostServer();
		vm = await AgentOs.create({ permissions: { network: "deny" } });
		await vm.writeFile("/app/server.mjs", SERVER_SCRIPT);
		await vm.writeFile("/app/probe.mjs", PROBE_SCRIPT);
		const serverPid = await startGuestServer(vm);

		// The host reaches the guest server over VM-local loopback.
		const response = await vm.network.httpRequest({
			port: 3000,
			path: "/from-host",
		});
		expect(response.status).toBe(200);
		expect(new TextDecoder().decode(response.body)).toBe("guest-ok:/from-host");

		const result = await runProbe(vm, hostPort);
		// Guest loopback reaches the guest's own listener.
		expect(result.vmLocal).toEqual({ status: 200, body: "guest-ok:/self" });
		// External egress is denied by the network policy.
		expect(result.external.error).toBe("EACCES");
		// Guest loopback is VM-local, so loopback confinement keeps it from
		// reaching host loopback services without an explicit exemption. No VM
		// listener owns the port, so the guest sees a refused connection, as in a
		// container with no published ports.
		expect(result.hostLoopback.error).toBe("ECONNREFUSED");
		expect(result.hostLoopback.message).toContain("loopbackExemptPorts");
		expect(hostRequests).toBe(0);

		await vm.process.kill(serverPid);
		await vm.process.wait(serverPid);
	}, 60_000);

	test("default policy keeps the full IPv4 loopback range VM-local", async () => {
		const hostPort = await startHostServer();
		// The Vitest setup substitutes allow-all when permissions is omitted;
		// an empty object reaches the sidecar's real per-scope defaults.
		vm = await AgentOs.create({ permissions: {} });
		await vm.writeFile(
			"/app/server.mjs",
			SERVER_SCRIPT.replace(
				'server.listen(3000, "127.0.0.1"',
				'server.listen(3000, "0.0.0.0"',
			),
		);
		await vm.writeFile(
			"/app/probe.mjs",
			PROBE_SCRIPT.replace("127.0.0.1:3000/self", "127.0.0.2:3000/self"),
		);
		const serverPid = await startGuestServer(vm);

		const result = await runProbe(vm, hostPort);
		expect(result.vmLocal).toEqual({ status: 200, body: "guest-ok:/self" });
		expect(result.external.error).toBe("EACCES");
		expect(result.hostLoopback.error).toBe("ECONNREFUSED");
		expect(hostRequests).toBe(0);

		await vm.process.kill(serverPid);
		await vm.process.wait(serverPid);
	}, 60_000);

	test("an explicit rule set can still deny VM-local networking", async () => {
		vm = await AgentOs.create({
			permissions: { network: { default: "deny", rules: [] } },
		});
		await vm.writeFile("/app/server.mjs", SERVER_SCRIPT);

		const decoder = new TextDecoder();
		let stdout = "";
		const { pid } = await vm.process.spawn("node", ["/app/server.mjs"], {
			onStdout: (chunk) => {
				stdout += decoder.decode(chunk);
			},
		});
		const exit = await vm.process.wait(pid);
		expect(exit.exitCode, stdout).toBe(3);
		expect(stdout).toContain("LISTEN_ERROR EACCES");
		expect(stdout).toContain("network.listen");
	}, 60_000);

	test("keeps VM-local UDP and Unix sockets under scalar deny", async () => {
		vm = await AgentOs.create({ permissions: { network: "deny" } });
		await vm.writeFile("/app/local-sockets.mjs", LOCAL_SOCKET_SCRIPT);
		const decoder = new TextDecoder();
		let stdout = "";
		let stderr = "";
		const { pid } = await vm.process.spawn("node", ["/app/local-sockets.mjs"], {
			onStdout: (chunk) => {
				stdout += decoder.decode(chunk);
			},
			onStderr: (chunk) => {
				stderr += decoder.decode(chunk);
			},
		});
		const exit = await vm.process.wait(pid);
		expect(exit.exitCode, stderr).toBe(0);
		expect(JSON.parse(stdout.trim().split("\n").at(-1) ?? "")).toEqual({
			udpBody: "udp-ok",
			unixBody: "unix-ok",
		});
	}, 60_000);

	test.each([
		{ name: "default policy", permissions: {} },
		{
			name: "explicit network allow",
			permissions: { network: "allow" as const },
		},
	])("mapped IPv6 loopback reaches the guest under $name", async ({
		permissions,
	}) => {
		const hostPort = await startHostServer();
		vm = await AgentOs.create({ permissions });
		await vm.writeFile(
			"/app/server.mjs",
			SERVER_SCRIPT.replace("3000", String(hostPort)),
		);
		await vm.writeFile("/app/raw-probe.mjs", RAW_LOOPBACK_PROBE_SCRIPT);
		const serverPid = await startGuestServer(vm);
		try {
			const result = await runRawTcpProbe(vm, hostPort);
			expect(result.exitCode, result.stderr).toBe(0);
			expect(result.response).toContain("guest-ok:/self");
			expect(hostRequests).toBe(0);
		} finally {
			await vm.process.kill(serverPid);
			await vm.process.wait(serverPid);
		}
	}, 60_000);

	test.each([
		{ name: "default policy", permissions: {} },
		{
			name: "explicit network allow",
			permissions: { network: "allow" as const },
		},
	])("a guest UDP port cannot grant TCP access to a same-port host under $name", async ({
		permissions,
	}) => {
		const hostPort = await startHostServer();
		vm = await AgentOs.create({ permissions });
		await vm.writeFile(
			"/app/server.mjs",
			GUEST_UDP_HOLD_SCRIPT.replace("%PORT%", String(hostPort)),
		);
		await vm.writeFile("/app/raw-probe.mjs", RAW_LOOPBACK_PROBE_SCRIPT);
		const serverPid = await startGuestServer(vm);
		try {
			const result = await runRawTcpProbe(vm, hostPort, "127.0.0.1");
			expect(hostRequests).toBe(0);
			expect(result.exitCode, result.stderr).toBe(2);
			expect(result.error).toMatch(/ECONNREFUSED|EACCES/);
		} finally {
			await vm.process.kill(serverPid);
			await vm.process.wait(serverPid);
		}
	}, 60_000);

	test("a guest TCP port cannot grant UDP access to a same-port host service", async () => {
		hostUdpServer = createSocket("udp4");
		hostUdpServer.on("message", () => {
			hostDatagrams += 1;
		});
		await new Promise<void>((resolve) =>
			hostUdpServer?.bind(0, "127.0.0.1", resolve),
		);
		const hostPort = (hostUdpServer.address() as AddressInfo).port;
		vm = await AgentOs.create({ permissions: { network: "allow" } });
		await vm.writeFile(
			"/app/server.mjs",
			SERVER_SCRIPT.replace("3000", String(hostPort)),
		);
		await vm.writeFile("/app/udp-send.mjs", GUEST_UDP_SEND_SCRIPT);
		const serverPid = await startGuestServer(vm);
		try {
			const decoder = new TextDecoder();
			let stdout = "";
			let stderr = "";
			const { pid } = await vm.process.spawn(
				"node",
				["/app/udp-send.mjs", String(hostPort)],
				{
					onStdout: (chunk) => {
						stdout += decoder.decode(chunk);
					},
					onStderr: (chunk) => {
						stderr += decoder.decode(chunk);
					},
				},
			);
			const exit = await vm.process.wait(pid);
			expect(exit.exitCode, stderr).toBe(0);
			await new Promise((resolve) => setTimeout(resolve, 100));
			expect(hostDatagrams).toBe(0);
			expect(JSON.parse(stdout.trim().split("\n").at(-1) ?? "").error).toMatch(
				/EACCES|EPERM/,
			);
		} finally {
			await vm.process.kill(serverPid);
			await vm.process.wait(serverPid);
		}
	}, 60_000);

	test("an explicit loopback exemption permits host-mediated mapped access", async () => {
		const hostPort = await startHostServer();
		vm = await AgentOs.create({
			permissions: { network: "allow" },
			loopbackExemptPorts: [hostPort],
		});
		await vm.writeFile("/app/raw-probe.mjs", RAW_LOOPBACK_PROBE_SCRIPT);
		const result = await runRawTcpProbe(vm, hostPort);
		expect(result.exitCode, result.stderr).toBe(0);
		expect(result.response).toContain("host-ok");
		expect(hostRequests).toBe(1);
	}, 60_000);

	test("mapped loopback cannot reach an unowned host port by default", async () => {
		const hostPort = await startHostServer();
		vm = await AgentOs.create({ permissions: {} });
		await vm.writeFile("/app/raw-probe.mjs", RAW_LOOPBACK_PROBE_SCRIPT);
		const result = await runRawTcpProbe(vm, hostPort);
		expect(result.exitCode, result.stderr).toBe(2);
		expect(result.error).toMatch(/ECONNREFUSED|EACCES/);
		expect(hostRequests).toBe(0);
	}, 60_000);
});
