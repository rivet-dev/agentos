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

// The exit event of P2 queues behind P1's flood on the event lane. Only the
// dispose response's list of finished executions can report its status. The
// flood stays under the TS client's 4096-event buffer.
test("a process that exits behind another process's output flood reports its real exit status on dispose()", async () => {
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

	// Block this thread while P2 exits, so the flood and P2's exit event both
	// wait unread when dispose() starts.
	Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 500);
	await vm.dispose();

	expect(exits).toEqual([{ pid, outcome: "exited", exitCode: 3 }]);
	// The flood was still running, so the dispose killed it.
	expect(floodExits).toEqual([
		{ pid: flood.pid, outcome: "signalled", signal: "SIGKILL" },
	]);
}, 60_000);
