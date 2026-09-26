import { vm } from "./client.js";

const conn = vm.connect();
await conn.ready;
const spawned = await conn.process.spawn({
	command: "node",
	args: ["/home/agentos/server.js"],
	options: { env: {} },
});

conn.on("process.output", (data) => {
	if (data.process.pid !== spawned.pid) return;
	// data.process: { generation, pid }
	// data.stream: "stdout" | "stderr"
	// data.data: Uint8Array
	const text = new TextDecoder().decode(data.data);
	console.log(`[${data.process.pid}] ${data.stream}: ${text}`);
});

conn.on("process.exit", (data) => {
	if (data.process.pid !== spawned.pid) return;
	// data.process: { generation, pid }
	// data.status.exitCode: number
	console.log(`Process ${data.process.pid} exited with code ${data.status.exitCode}`);
});
