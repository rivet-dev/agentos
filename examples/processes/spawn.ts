import { vm } from "./client.js";

const conn = vm.connect();
await conn.ready;

// Spawn a dev server
const spawned = await conn.process.spawn({
	command: "node",
	args: ["/home/agentos/server.js"],
	options: { env: {} },
});

// Subscribe to process output
conn.on("process.output", (data) => {
	if (data.process.pid !== spawned.pid) return;
	const text = new TextDecoder().decode(data.data);
	console.log(`[pid ${data.process.pid}] ${data.stream}: ${text}`);
});

conn.on("process.exit", (data) => {
	if (data.process.pid !== spawned.pid) return;
	console.log(`[pid ${data.process.pid}] exited with code ${data.status.exitCode}`);
});

console.log("Started process:", spawned.pid);
