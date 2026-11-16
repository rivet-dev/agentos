import { expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import type { ProcessExit } from "../src/language-execution.js";

test("a process that exits just before dispose() reports its real exit status", async () => {
	const vm = await AgentOs.create();
	const { pid } = await vm.javascript.spawn(
		"setTimeout(() => process.exit(3), 20)",
	);
	const exits: ProcessExit[] = [];
	vm.onProcessExit(pid, (exit) => exits.push(exit));

	// Block this thread while the guest exits, so its completion event still
	// waits unread on the event lane when dispose() starts.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
	await vm.dispose();

	expect(exits).toEqual([{ pid, outcome: "exited", exitCode: 3 }]);
}, 30_000);

// Warm-path check: a process that exits just before dispose() while another
// process prints keeps its real exit status, and the printing process reports
// the dispose's SIGKILL. Here P2's exit event still reaches the client on the
// event lane, so this does not cover the sidecar's list of finished
// executions in the dispose response.
test("a process that exits while another process prints reports its real exit status on dispose()", async () => {
	// Warm the shared sidecar inside the test, so P2's exit is due well before
	// the dispose whatever ran first.
	const warmup = await AgentOs.create();
	await warmup.dispose();

	const vm = await AgentOs.create();
	const flood = await vm.javascript.spawn(
		"for (let i = 0; i < 3000; i++) console.log('flood ' + i); setInterval(() => {}, 1000)",
	);
	const { pid } = await vm.javascript.spawn(
		"setTimeout(() => process.exit(3), 100)",
	);
	const exits: ProcessExit[] = [];
	vm.onProcessExit(pid, (exit) => exits.push(exit));
	const floodExits: ProcessExit[] = [];
	vm.onProcessExit(flood.pid, (exit) => floodExits.push(exit));

	// Block this thread while P2 exits, so its exit event waits unread when
	// dispose() starts.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
	await vm.dispose();

	expect(exits).toEqual([{ pid, outcome: "exited", exitCode: 3 }]);
	// The flood was still running, so the dispose killed it.
	expect(floodExits).toEqual([
		{ pid: flood.pid, outcome: "signalled", signal: "SIGKILL" },
	]);
}, 60_000);
