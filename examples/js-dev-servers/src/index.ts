import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const serverSource = `
import http from "node:http";
const app = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "application/json" });
  res.end(JSON.stringify({ ok: true, path: req.url }));
});
app.listen(3000, "127.0.0.1", () => console.log("ready"));
await new Promise(() => {});
`;

const runtime = await JavaScriptRuntime.create({
	permissions: { network: "allow" },
});

let ready!: () => void;
const serverReady = new Promise<void>((resolve) => {
	ready = resolve;
});

try {
	const server = await runtime.execute(serverSource, {
		detached: true,
		onStdout: (chunk) => {
			const text = new TextDecoder().decode(chunk);
			process.stdout.write(text);
			if (text.includes("ready")) ready();
		},
	});

	await serverReady;
	const response = await runtime.vm.network.httpRequest({
		port: 3000,
		path: "/health",
	});
	console.log(response.status, new TextDecoder().decode(response.body));

	await runtime.vm.executions.signal(server.executionId, "SIGTERM");
	await runtime.vm.executions.wait(server.executionId);
} finally {
	await runtime.dispose();
}
