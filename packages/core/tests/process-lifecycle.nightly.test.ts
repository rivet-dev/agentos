import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { ALLOW_ALL_VM_PERMISSIONS } from "./helpers/permissions.js";

function normalizeLifecycleError(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function isExpectedTeardownError(message: string): boolean {
	const normalized = message.toLowerCase();
	return (
		normalized.includes("err_agentos_request_admission_closed") ||
		normalized.includes("unknown sidecar vm") ||
		normalized.includes("already been disposed") ||
		normalized.includes("sidecar disposed") ||
		normalized.includes("cannot dispatch request on closed sidecar process")
	);
}

describe("process lifecycle teardown races", () => {
	let vm: AgentOs | undefined;

	beforeEach(async () => {
		vm = await AgentOs.create({ permissions: ALLOW_ALL_VM_PERMISSIONS });
	});

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("filesystem calls racing vm.dispose() settle without transport crashes", async () => {
		if (!vm) {
			throw new Error("vm should be created before test execution");
		}
		await vm.writeFile(
			"/tmp/hold-open.mjs",
			"console.log('ready'); setInterval(() => {}, 1_000);",
		);
		await vm.writeFile("/tmp/seed.txt", "seed");

		let child: ReturnType<AgentOs["spawn"]> | undefined;
		let readyTimer: ReturnType<typeof setTimeout> | undefined;
		const ready = new Promise<void>((resolve, reject) => {
			readyTimer = setTimeout(
				() => reject(new Error("Node child did not start")),
				2_000,
			);
			const process = vm!.spawn("node", ["/tmp/hold-open.mjs"], {
				env: { HOME: "/home/agentos" },
				onStdout: (chunk) => {
					if (new TextDecoder().decode(chunk).includes("ready")) resolve();
				},
			});
			child = process;
		});
		await ready.finally(() => {
			if (readyTimer !== undefined) clearTimeout(readyTimer);
		});

		if (!child) throw new Error("Node child was not created");
		const admittedChild = child;
		const childExit = vm.process.wait(admittedChild.pid);

		const operations = Array.from({ length: 12 }, (_, index) =>
			(index % 3 === 0
				? vm.writeFile(`/tmp/race-${index}.txt`, `payload-${index}`)
				: index % 3 === 1
					? vm.readFile("/tmp/seed.txt")
					: vm.stat("/tmp/seed.txt")
			).then(
				(value) => ({ ok: true as const, value }),
				(error) => ({
					ok: false as const,
					message: normalizeLifecycleError(error),
				}),
			),
		);

		const disposePromise = vm.dispose();
		const results = await Promise.all(operations);
		await disposePromise;
		expect([143, 137]).toContain((await childExit).exitCode);

		for (const result of results) {
			if (result.ok) {
				continue;
			}
			expect(
				isExpectedTeardownError(result.message),
				`unexpected teardown error: ${result.message}`,
			).toBe(true);
		}

		await vm.readFile("/tmp/seed.txt").then(
			() => {
				throw new Error("disposed VM unexpectedly accepted a read");
			},
			(error) => {
				const message = normalizeLifecycleError(error);
				expect(isExpectedTeardownError(message), message).toBe(true);
			},
		);
	}, 30_000);

	test("idle retained contexts release one VM without disrupting peers", async () => {
		if (!vm) throw new Error("vm should be created before test execution");
		let firstPeer: AgentOs | undefined;
		let secondPeer: AgentOs | undefined;
		try {
			[firstPeer, secondPeer] = await Promise.all([
				AgentOs.create({ permissions: ALLOW_ALL_VM_PERMISSIONS }),
				AgentOs.create({ permissions: ALLOW_ALL_VM_PERMISSIONS }),
			]);
			if (!firstPeer || !secondPeer)
				throw new Error("peer VMs were not created");
			for (const current of [vm, firstPeer, secondPeer]) {
				await current.createContext("retained");
				const result = await current.javascript.evaluate("6 * 7", {
					contextId: "retained",
				});
				expect(result.value).toBe(42);
			}
			await firstPeer.dispose();
			firstPeer = undefined;
			expect(
				(
					await secondPeer.javascript.evaluate("6 * 7", {
						contextId: "retained",
					})
				).value,
			).toBe(42);
			await secondPeer.dispose();
			secondPeer = undefined;
			expect(
				(await vm.javascript.evaluate("6 * 7", { contextId: "retained" }))
					.value,
			).toBe(42);
		} finally {
			await firstPeer?.dispose();
			await secondPeer?.dispose();
		}
	}, 30_000);
});
