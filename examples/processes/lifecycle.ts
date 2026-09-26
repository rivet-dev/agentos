import { vm } from "./client.js";

const process = await vm.process.spawn({
	command: "node",
	args: ["/home/agentos/server.js"],
	options: { env: {} },
});

const processStatus = (process: {
	running: boolean;
	exit?: { exitCode: number } | null;
}) => (process.running ? "running" : `exited ${process.exit?.exitCode ?? ""}`.trim());

// List all processes tracked by the VM
const processes = await vm.process.list({});
for (const p of processes) {
	console.log(p.process.pid, p.command, p.args.join(" "), processStatus(p));
}

// Inspect a specific process by pid
const info = await vm.process.get({ process });
console.log(processStatus(info), info.exit?.exitCode);

// Graceful stop (SIGTERM)
await vm.process.signal({ process, signal: "SIGTERM" });

// Force kill (SIGKILL)
await vm.process.signal({ process, signal: "SIGKILL" });
