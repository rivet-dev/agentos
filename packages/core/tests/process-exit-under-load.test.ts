import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

const vms: AgentOs[] = [];

afterEach(async () => {
	await Promise.all(vms.splice(0).map((vm) => vm.dispose()));
});

// Starts `code` as a Node guest in its own VM and resolves once it prints "up".
async function startGuest(code: string) {
	const vm = await AgentOs.create();
	vms.push(vm);
	let stdout = "";
	let wake = () => {};
	const { pid } = await vm.process.spawn("node", ["-e", code], {
		onStdout: (chunk) => {
			if (!stdout.includes("up")) stdout += Buffer.from(chunk).toString();
			wake();
		},
	});
	const exited = vm.process.wait(pid);
	while (!stdout.includes("up")) {
		await new Promise<void>((resolve) => {
			wake = resolve;
		});
	}
	return { vm, pid, exited };
}

// Resolves with the milliseconds from SIGKILL to the observed exit.
async function sigkill({
	vm,
	pid,
	exited,
}: Awaited<ReturnType<typeof startGuest>>) {
	const killed = performance.now();
	await vm.process.signal(pid, "SIGKILL");
	expect(await exited).toMatchObject({
		outcome: "signalled",
		signal: "SIGKILL",
	});
	return performance.now() - killed;
}

const busyGuest =
	"process.stdout.write('up\\n'); setTimeout(() => { for (;;) {} }, 0);";
const streamingGuest =
	"process.stdout.write('up\\n'); setInterval(() => { for (let i = 0; i < 20; i++) process.stdout.write('line\\n'); }, 1);";

test("a SIGKILLed busy guest exits promptly while other VMs stream output", async () => {
	const busy = await Promise.all(
		Array.from({ length: 8 }, () => startGuest(busyGuest)),
	);
	const streaming = await Promise.all(
		Array.from({ length: 8 }, () => startGuest(streamingGuest)),
	);
	// Let the streams fill the sidecar's event queues before the kills.
	await new Promise((resolve) => setTimeout(resolve, 300));

	const exitMs = await Promise.all(busy.map(sigkill));
	expect(Math.max(...exitMs)).toBeLessThan(1000);
	// The streaming VMs kept running and stop the same way.
	await Promise.all(streaming.map(sigkill));
}, 120_000);
