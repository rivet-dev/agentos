import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

const vms: AgentOs[] = [];

afterEach(async () => {
	await Promise.all(vms.splice(0).map((vm) => vm.dispose()));
});

test("every byte written before exit reaches onStdout when the host learns the exit early", async () => {
	const vm = await AgentOs.create();
	vms.push(vm);
	const chunk = 16 * 1024;
	const count = 2048;
	let received = 0;
	const { pid } = await vm.process.spawn(
		"node",
		[
			"-e",
			`const b = Buffer.alloc(${chunk}, 120); for (let i = 0; i < ${count}; i++) process.stdout.write(b);`,
		],
		{
			output: { retainEvents: true },
			// A slow consumer: the host takes a while to drain queued output.
			onStdout: (bytes) => {
				received += bytes.length;
				const until = performance.now() + 0.3;
				while (performance.now() < until) {}
			},
		},
	);
	// The host stays busy while the guest writes everything and exits, so the
	// output waits in the event lane. Replay answers on the response lane, so
	// it reports the exit while that output is still queued. Poll until it does
	// instead of guessing how long a slow machine needs.
	const busy = new Int32Array(new SharedArrayBuffer(4));
	let exitCode: number | null = null;
	const exitDeadline = Date.now() + 30_000;
	while (exitCode === null && Date.now() < exitDeadline) {
		Atomics.wait(busy, 0, 0, 100);
		exitCode = (await vm.process.readOutput(pid, { maxEvents: 1 })).exitCode;
	}
	expect(exitCode).toBe(0);
	// The case under test: the exit is known before all output is delivered.
	expect(received).toBeLessThan(chunk * count);
	expect(await vm.process.wait(pid)).toMatchObject({ outcome: "exited", exitCode: 0 });

	// Like a pipe reader on Linux, the listener gets every byte, none dropped.
	const deadline = Date.now() + 20_000;
	while (received < chunk * count && Date.now() < deadline) {
		await new Promise((resolve) => setTimeout(resolve, 50));
	}
	expect(received).toBe(chunk * count);
}, 90_000);
