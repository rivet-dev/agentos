import { AgentOs } from "@rivet-dev/agentos-core";

const serverSource = `
import http from "node:http";
const app = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({ ok: true, path: req.url }));
});
app.listen(3000, "127.0.0.1", () => console.log("ready"));
await new Promise(() => {});
`;

const runtime = await AgentOs.create({ permissions: { network: "allow" } });

try {
	// `spawn` returns as soon as the process starts, before the server is
	// listening. Wait for the line it prints once it is ready.
	const ready = Promise.withResolvers<void>();
	const decoder = new TextDecoder();
	const server = await runtime.javascript.spawn(serverSource, {
		onStdout: (chunk) => {
			const text = decoder.decode(chunk);
			process.stdout.write(text);
			if (text.includes("ready")) ready.resolve();
		},
	});
	await ready.promise;

	// The listener stays inside the VM. No host port is exposed.
	const response = await runtime.network.httpRequest({
		port: 3000,
		path: "/health",
	});
	console.log(response.status); // 200

	await runtime.process.signal(server.pid, "SIGTERM");
	await runtime.process.wait(server.pid);
} finally {
	await runtime.dispose();
}
