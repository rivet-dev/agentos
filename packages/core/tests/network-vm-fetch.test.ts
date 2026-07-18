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
			'  console.log(`LISTENING:${server.address().port}`);',
			"});",
		].join("\n"),
	);

	let resolvePort!: (port: number) => void;
	const portPromise = new Promise<number>((resolve) => {
		resolvePort = resolve;
	});

	const { pid } = vm.spawn("node", ["/tmp/server.js"]);
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
		vm.stopProcess(pid);
		await vm.waitProcess(pid).catch(() => {});
	}
});
