import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const textDecoder = new TextDecoder();

let vm: AgentOs | null = null;

afterEach(async () => {
	await vm?.dispose();
	vm = null;
});

test("httpRequest reaches a guest http.createServer listener", async () => {
	vm = await AgentOs.create({
		permissions: {
			fs: "allow",
			network: "allow",
			childProcess: "allow",
		},
	});

	await vm.writeFile(
		"/tmp/server.js",
		[
			'const http = require("node:http");',
			"const server = http.createServer((req, res) => {",
			'  res.writeHead(200, { "Content-Type": "application/json" });',
			'  res.end(JSON.stringify({ status: "ok", method: req.method, url: req.url }));',
			"});",
			'server.listen(0, "0.0.0.0", () => {',
			"  console.log(`LISTENING:${server.address().port}`);",
			"});",
		].join("\n"),
	);

	let resolvePort!: (port: number) => void;
	const portPromise = new Promise<number>((resolve) => {
		resolvePort = resolve;
	});

	const { pid } = await vm.process.spawn("node", ["/tmp/server.js"]);
	const unsubscribe = vm.onProcessOutput(pid, (event) => {
		if (event.stream === "stdout") {
			const chunk = event.data;
			const text = textDecoder.decode(chunk);
			const match = text.match(/LISTENING:(\d+)/);
			if (match) {
				resolvePort(Number(match[1]));
			}
		}
	});

	try {
		const guestPort = await portPromise;
		const response = await vm.httpRequest({
			port: guestPort,
			path: "/api/test",
		});

		expect(response.status).toBe(200);
		expect(JSON.parse(textDecoder.decode(response.body))).toEqual({
			status: "ok",
			method: "GET",
			url: "/api/test",
		});
	} finally {
		unsubscribe();
		await vm.process.signal(pid, "SIGTERM");
		await vm.process.wait(pid).catch(() => {});
	}
});

test("a guest fetch with an unread body lets the process exit", async () => {
	vm = await AgentOs.create({ permissions: { network: "allow" } });
	// The server keeps the connection open after each response, so the client
	// holds an idle keep-alive socket and an unread body when its script ends.
	await vm.writeFile(
		"/app/server.mjs",
		[
			'import net from "node:net";',
			"const response =",
			'  "HTTP/1.1 200 OK\\r\\nContent-Length: 2\\r\\nConnection: keep-alive\\r\\nKeep-Alive: timeout=120\\r\\n\\r\\nok";',
			'const server = net.createServer((socket) => socket.on("data", () => socket.write(response)));',
			'server.listen(3000, "127.0.0.1", () => console.log("LISTENING"));',
		].join("\n"),
	);
	await vm.writeFile(
		"/app/client.mjs",
		'console.log((await fetch("http://127.0.0.1:3000/")).status);',
	);

	let listening!: () => void;
	const ready = new Promise<void>((resolve) => {
		listening = resolve;
	});
	const server = await vm.process.spawn("node", ["/app/server.mjs"], {
		onStdout: (chunk) => {
			if (textDecoder.decode(chunk).includes("LISTENING")) listening();
		},
	});
	try {
		await ready;
		let stdout = "";
		let stderr = "";
		const startedAt = Date.now();
		const client = await vm.process.spawn("node", ["/app/client.mjs"], {
			onStdout: (chunk) => {
				stdout += textDecoder.decode(chunk);
			},
			onStderr: (chunk) => {
				stderr += textDecoder.decode(chunk);
			},
		});
		const exit = await vm.process.wait(client.pid);
		const elapsedMs = Date.now() - startedAt;

		expect(exit.exitCode, stderr).toBe(0);
		expect(stdout.trim()).toBe("200");
		expect(elapsedMs).toBeLessThan(5_000);
	} finally {
		await vm.process.kill(server.pid);
		await vm.process.wait(server.pid).catch(() => {});
	}
}, 60_000);
