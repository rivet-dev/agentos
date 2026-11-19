import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

// All VMs created by one host share one sidecar and its outbound event queue.
const vms: AgentOs[] = [];

afterEach(async () => {
	await Promise.all(vms.splice(0).map((vm) => vm.dispose()));
});

const LINES = 20_000;
// Writes 0..lines-1, one line per write, as fast as the guest can.
const floodGuest = (lines: number) =>
	`for (let i = 0; i < ${lines}; i++) process.stdout.write(i + "\\n");`;
const expectedFlood = Array.from({ length: LINES }, (_, i) => `${i}\n`).join("");

async function startFlood(lines = LINES) {
	const vm = await AgentOs.create();
	vms.push(vm);
	let stdout = "";
	const { pid } = await vm.process.spawn("node", ["-e", floodGuest(lines)], {
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
	});
	return {
		vm,
		done: vm.process.wait(pid).then((exit) => ({ exit, stdout })),
	};
}

// A guest that echoes stdin lines, for request/response round trips.
async function startEcho() {
	const vm = await AgentOs.create();
	vms.push(vm);
	let stdout = "";
	let wake = () => {};
	const { pid } = await vm.process.spawn(
		"node",
		[
			"-e",
			"process.stdin.on('data', (d) => process.stdout.write(d)); process.stdout.write('up\\n');",
		],
		{
			onStdout: (chunk) => {
				stdout += Buffer.from(chunk).toString();
				wake();
			},
		},
	);
	const until = async (text: string) => {
		while (!stdout.includes(text)) {
			await new Promise<void>((resolve) => {
				wake = resolve;
			});
		}
	};
	await until("up\n");
	return {
		vm,
		pid,
		// Resolves with the milliseconds one echo round trip took.
		async roundTrip(line: string) {
			const sent = performance.now();
			await vm.process.writeStdin(pid, `${line}\n`);
			await until(`${line}\n`);
			return performance.now() - sent;
		},
	};
}

test("guests flooding stdout cannot stall or disconnect another VM", async () => {
	const echo = await startEcho();
	const floods = await Promise.all(Array.from({ length: 8 }, startFlood));

	const latencies: number[] = [];
	for (let i = 0; i < 20; i++) latencies.push(await echo.roundTrip(`ping-${i}`));
	await echo.vm.process.closeStdin(echo.pid);
	const echoExit = echo.vm.process.wait(echo.pid);

	// The bystander's exit arrives while the floods are still draining.
	expect(await echoExit).toMatchObject({ outcome: "exited", exitCode: 0 });
	expect(Math.max(...latencies)).toBeLessThan(2_000);
	// Every flood finishes with its full output in order: paused, never dropped.
	for (const { done } of floods) {
		const { exit, stdout } = await done;
		expect(exit).toMatchObject({ outcome: "exited", exitCode: 0 });
		expect(stdout === expectedFlood).toBe(true);
	}
}, 120_000);

test("a host that stops reading output does not disconnect any VM", async () => {
	const floods = await Promise.all(Array.from({ length: 8 }, startFlood));
	// Block the host event loop: nothing reads the sidecar's output while
	// the guests keep writing.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2_000);

	for (const { done } of floods) {
		const { exit, stdout } = await done;
		expect(exit).toMatchObject({ outcome: "exited", exitCode: 0 });
		expect(stdout === expectedFlood).toBe(true);
	}
	// The shared transport still serves new work.
	const echo = await startEcho();
	expect(await echo.roundTrip("after")).toBeLessThan(2_000);
	await echo.vm.process.closeStdin(echo.pid);
	expect(await echo.vm.process.wait(echo.pid)).toMatchObject({
		outcome: "exited",
		exitCode: 0,
	});
}, 120_000);

test("disposing a flooding VM resumes another VM's paused output", async () => {
	const flooder = await startFlood(200_000);
	const paused = await startFlood();
	// Stop reading so both VMs fill their share of the shared queue and pause.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 1_000);

	// The disposed VM's own process ends with its VM; only the other VM's
	// outcome is under test.
	const flooderDone = flooder.done.catch((error: unknown) => error);
	vms.splice(vms.indexOf(flooder.vm), 1);
	await flooder.vm.dispose();
	await flooderDone;

	const { exit, stdout } = await paused.done;
	expect(exit).toMatchObject({ outcome: "exited", exitCode: 0 });
	expect(stdout === expectedFlood).toBe(true);
	// The shared transport still serves new work.
	const echo = await startEcho();
	expect(await echo.roundTrip("after")).toBeLessThan(2_000);
	await echo.vm.process.closeStdin(echo.pid);
	expect(await echo.vm.process.wait(echo.pid)).toMatchObject({
		outcome: "exited",
		exitCode: 0,
	});
}, 120_000);

test("one write larger than the VM's event budget drains in chunks", async () => {
	const budget = 16 * 1024;
	const vm = await AgentOs.create({ limits: { process: { pendingEventBytes: budget } } });
	vms.push(vm);
	const size = 16 * budget;
	let received = Buffer.alloc(0);
	const { pid } = await vm.process.spawn(
		"node",
		["-e", `process.stdout.write(Buffer.alloc(${size}, "abcdefgh"));`],
		{
			onStdout: (chunk) => {
				received = Buffer.concat([received, Buffer.from(chunk)]);
			},
		},
	);
	// Like a large write to a full pipe on Linux: the writer is not failed;
	// its data arrives whole and in order.
	expect(await vm.process.wait(pid)).toMatchObject({ outcome: "exited", exitCode: 0 });
	expect(received.equals(Buffer.alloc(size, "abcdefgh"))).toBe(true);
}, 60_000);
