import { createServer, type Server } from "node:http";
import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

let server: Server | null = null;
let vm: AgentOs | null = null;

// Keep this regression isolated in its own Vitest worker so the sidecar's
// process-global JavaScript timer wheel is first initialized by this fetch.
afterEach(async () => {
	await vm?.dispose();
	vm = null;
	if (server?.listening) {
		await new Promise<void>((resolve, reject) => {
			server?.close((error) => (error ? reject(error) : resolve()));
		});
	}
	server = null;
});

test("disposes promptly after guest fetch consumes the response body", async () => {
	server = createServer((_request, response) => {
		response.writeHead(200, { "content-type": "text/plain" });
		response.end("ok");
	});
	await new Promise<void>((resolve) => {
		server?.listen(0, "127.0.0.1", resolve);
	});
	const address = server.address();
	if (!address || typeof address === "string") {
		throw new Error("local HTTP fixture did not expose a TCP port");
	}

	vm = await AgentOs.create({
		defaultSoftware: false,
		loopbackExemptPorts: [address.port],
		permissions: {
			fs: "allow",
			network: "allow",
			childProcess: "allow",
		},
	});

	const result = await vm.javascript.evaluate<string>(
		`(async () => {
			const response = await fetch("http://127.0.0.1:${address.port}/dispose");
			return await response.text();
		})()`,
	);
	expect(result).toMatchObject({ outcome: "succeeded", value: "ok" });

	const disposeStartedAt = performance.now();
	await vm.dispose();
	vm = null;
	const disposeDurationMs = performance.now() - disposeStartedAt;

	expect(disposeDurationMs).toBeLessThan(4_000);
}, 30_000);
