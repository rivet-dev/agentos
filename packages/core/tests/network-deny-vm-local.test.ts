import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

// A scalar `network: "deny"` denies traffic that crosses the VM boundary. Like
// the default policy, it keeps guest listeners and VM-local loopback working,
// so a VM behaves like a container with no published ports.

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
	probe("http://example.com/"),
	probe("http://127.0.0.1:" + process.argv[2] + "/host"),
]);
console.log(JSON.stringify({ vmLocal, external, hostLoopback }));
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

describe('scalar network "deny"', () => {
	let vm: AgentOs | undefined;
	let hostServer: Server | undefined;
	let hostRequests = 0;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
		await new Promise<void>((resolve) => {
			if (!hostServer) return resolve();
			hostServer.close(() => resolve());
		});
		hostServer = undefined;
		hostRequests = 0;
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
});
