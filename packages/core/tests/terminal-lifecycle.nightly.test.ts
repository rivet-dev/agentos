import { describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const PROMPT = "sh-0.4$ ";
const OUTPUT_DEADLINE_MS = 5_000;
const SOAK_ROUNDS = Number(process.env.AGENTOS_TERMINAL_SOAK_ROUNDS ?? 3);
const VM_COUNT = 2;

function outputWaiter(vm: AgentOs, shellId: string) {
	let output = "";
	const waiters = new Set<{ text: string; resolve: () => void }>();
	const decoder = new TextDecoder();
	const unsubscribe = vm.onShellData(shellId, ({ data }) => {
		output += decoder.decode(data, { stream: true });
		for (const waiter of waiters) {
			if (output.includes(waiter.text)) {
				waiters.delete(waiter);
				waiter.resolve();
			}
		}
	});
	return {
		waitFor(text: string): Promise<void> {
			if (output.includes(text)) return Promise.resolve();
			return new Promise<void>((resolve, reject) => {
				const waiter = {
					text,
					resolve: () => {
						clearTimeout(deadline);
						resolve();
					},
				};
				const deadline = setTimeout(() => {
					waiters.delete(waiter);
					reject(
						new Error(
							`terminal output deadline waiting for ${text}: ${output}`,
						),
					);
				}, OUTPUT_DEADLINE_MS);
				waiters.add(waiter);
			});
		},
		get output() {
			return output;
		},
		dispose() {
			unsubscribe();
		},
	};
}

function flattenProcessTree(
	nodes: Awaited<ReturnType<AgentOs["process"]["tree"]>>,
) {
	return nodes.flatMap((node) => [node, ...flattenProcessTree(node.children)]);
}

describe("native terminal lifecycle", () => {
	test("repeated default shells exit and release process and VM counts across peers", async () => {
		if (
			!Number.isSafeInteger(SOAK_ROUNDS) ||
			SOAK_ROUNDS < 1 ||
			SOAK_ROUNDS > 64
		) {
			throw new Error(
				"AGENTOS_TERMINAL_SOAK_ROUNDS must be an integer from 1 through 64",
			);
		}
		const sidecar = await AgentOs.createSidecar();
		const vms: AgentOs[] = [];
		try {
			for (let peer = 0; peer < VM_COUNT; peer++) {
				vms.push(
					await AgentOs.create({
						sidecar: { kind: "explicit", handle: sidecar },
					}),
				);
			}
			expect(sidecar.describe().activeVmCount).toBe(VM_COUNT);
			for (let round = 0; round < SOAK_ROUNDS; round++) {
				const shells = vms.map((vm) => {
					const { shellId } = vm.terminal.open();
					return { vm, shellId, output: outputWaiter(vm, shellId) };
				});
				try {
					await Promise.all(shells.map(({ output }) => output.waitFor(PROMPT)));
					for (let peer = 0; peer < shells.length; peer++) {
						const { vm, shellId, output } = shells[peer];
						const marker = `__TERMINAL_DONE_${round}_${peer}__`;
						await vm.terminal.write(
							shellId,
							`printf '%s%s\\n' '__TERMINAL_DONE_' '${round}_${peer}__'\n`,
						);
						await output.waitFor(marker);
						const active = flattenProcessTree(await vm.process.tree()).filter(
							(process) =>
								process.state === "running" && process.command === "sh",
						);
						expect(active.length, output.output).toBe(1);
					}
					await Promise.all(
						shells.map(async ({ vm, shellId }) => {
							const exit = vm.terminal.wait(shellId);
							if (round % 2 === 0) {
								await vm.terminal.write(shellId, "exit\n");
								expect(await exit).toBe(0);
								vm.terminal.close(shellId);
							} else {
								vm.terminal.close(shellId);
								expect([137, 143]).toContain(await exit);
							}
							// Terminal exit can precede the kernel process snapshot update. The
							// public API has no separate readiness event for that snapshot.
							await expect
								.poll(
									async () =>
										flattenProcessTree(await vm.process.tree()).filter(
											(process) => process.state !== "exited",
										).length,
									{ timeout: 1_000, interval: 10 },
								)
								.toBe(0);
							const processes = flattenProcessTree(await vm.process.tree());
							expect(
								processes.some(
									(process) =>
										process.state === "exited" && process.command === "sh",
								),
							).toBe(true);
						}),
					);
				} finally {
					for (const { output } of shells) {
						output.dispose();
					}
				}
			}
		} finally {
			try {
				await Promise.all(vms.map((vm) => vm.dispose()));
				expect(sidecar.describe().activeVmCount).toBe(0);
			} finally {
				await sidecar.dispose();
			}
		}
	}, 60_000);
});
