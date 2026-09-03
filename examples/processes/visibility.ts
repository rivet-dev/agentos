import { vm } from "./client.js";

const processStatus = (process: {
	running: boolean;
	exit?: { exitCode: number } | null;
}) => (process.running ? "running" : `exited ${process.exit?.exitCode ?? ""}`.trim());

// All processes spawned in the VM
const all = await vm.process.list({});
for (const p of all) {
	console.log(p.process.pid, p.command, p.args.join(" "), processStatus(p));
}

// Inspect a single process by pid
const first = all[0];
if (first) {
	const info = await vm.process.get({ process: first.process });
	console.log(info.process.pid, info.command, "status:", processStatus(info));
}
