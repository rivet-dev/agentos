import { expect, onTestFinished, test } from "vitest";
import { z } from "zod";
import { AgentOs } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

// An explicit sidecar runs the native runtime in its own child process. A
// callback lifetime regression must reject this test, never crash Vitest.
for (const count of [1, 2]) {
	test(`retained Python bridge survives repeated calls across ${count} VMs`, async () => {
		const sidecar = await AgentOs.createSidecar();
		const vms: AgentOs[] = [];
		onTestFinished(async () => {
			const results = await Promise.allSettled(vms.map((vm) => vm.dispose()));
			results.push(...(await Promise.allSettled([sidecar.dispose()])));
			const errors = results.flatMap((result) =>
				result.status === "rejected" ? [result.reason] : [],
			);
			if (errors.length) {
				throw new AggregateError(errors, "Retained bridge test cleanup failed");
			}
		});
		for (let slot = 0; slot < count; slot++) {
			const vm = await AgentOs.create({
				sidecar: { kind: "explicit", handle: sidecar },
				defaultSoftware: false,
				permissions: ALLOW_ALL_VM_PERMISSIONS,
			});
			vms.push(vm);
			const cold = await vm.process.execFile(
				"python",
				["-c", "print('ready')"],
				{
					timeoutMs: 10_000,
					output: { capture: "all", maxBufferedBytes: 4096 },
				},
			);
			expect(cold.exitCode, cold.stderr).toBe(0);
			expect(cold.stdout).toBe("ready\n");
			await vm.createContext("retained");
			await vm.writeFile("/tmp/owner", String(slot));
		}

		async function repeat(vm: AgentOs, slot: number, iteration: number) {
			const result = await vm.python.execute(
				`import os
with open('/tmp/owner') as owner:
    assert owner.read() == '${slot}'
fd = os.open('/tmp/retained-fd', os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o600)
try:
    assert os.write(fd, b'${slot}:${iteration}') == 3
    assert os.lseek(fd, 0, os.SEEK_SET) == 0
    assert os.read(fd, 3) == b'${slot}:${iteration}'
finally:
    os.close(fd)
os.unlink('/tmp/retained-fd')
print('ok')`,
				{
					contextId: "retained",
					timeoutMs: 10_000,
					output: { capture: "all" },
				},
			);
			expect(result.outcome, result.stderr).toBe("succeeded");
			expect(result.stdout).toBe("ok\n");
			expect(result.stderr).toBe("");
		}

		for (let iteration = 0; iteration < 3; iteration++) {
			await Promise.all(vms.map((vm, slot) => repeat(vm, slot, iteration)));
		}
		if (count === 2) {
			const [disposed, survivor] = vms;
			if (!disposed || !survivor) throw new Error("Expected two test VMs");
			await disposed.dispose();
			await repeat(survivor, 1, 3);
		}
	}, 60_000);
}

test("saved JavaScript host callbacks survive retained execution boundaries", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	onTestFinished(async () => {
		try {
			await vm?.dispose();
		} finally {
			await sidecar.dispose();
		}
	});
	vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
		defaultSoftware: false,
		permissions: ALLOW_ALL_VM_PERMISSIONS,
		hostFunctions: {
			counter: {
				increment: {
					inputSchema: z.object({ value: z.number() }),
					execute: ({ value }) => ({ value: value + 1 }),
				},
			},
		},
	});
	await vm.createContext("saved-callback");
	const saved = await vm.javascript.execute(
		"globalThis.savedIncrement = counter.increment; console.log((await savedIncrement({ value: 0 })).value)",
		{
			contextId: "saved-callback",
			timeoutMs: 10_000,
			output: { capture: "all" },
		},
	);
	expect(saved.outcome, saved.stderr).toBe("succeeded");
	expect(saved.stdout).toBe("1\n");
	for (const value of [1, 2]) {
		const result = await vm.javascript.execute(
			`console.log((await savedIncrement({ value: ${value} })).value)`,
			{
				contextId: "saved-callback",
				timeoutMs: 10_000,
				output: { capture: "all" },
			},
		);
		expect(result.outcome, result.stderr).toBe("succeeded");
		expect(result.stdout).toBe(`${value + 1}\n`);
	}
}, 30_000);
