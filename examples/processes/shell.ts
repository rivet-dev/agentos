import { vm } from "./client.js";

const conn = vm.connect();
await conn.ready;

// Spawn an interactive shell process
const spawned = await conn.process.spawn({
	command: "sh",
	args: [],
	options: { env: {} },
});

// Stream this process's output as it is produced
conn.on("process.output", (data) => {
	if (data.process.pid !== spawned.pid) return;
	const text = new TextDecoder().decode(data.data);
	process.stdout.write(text);
});

// Drive it by writing commands to stdin
await conn.process.stdin.write({
	process: spawned,
	data: "ls -la /home/agentos\n",
});

// Close stdin to let the shell exit, then wait for it
await conn.process.stdin.close({ process: spawned });
await conn.process.wait({ process: spawned });
