import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("retained call cleanup retires a refreshed timer before admitting the next call", async () => {
	const sidecar = await AgentOs.createSidecar();
	let vm: AgentOs | undefined;
	try {
		vm = await AgentOs.create({
			sidecar: { kind: "explicit", handle: sidecar },
			defaultSoftware: false,
			permissions: { network: "deny" },
			limits: { jsRuntime: { maxTimers: 1 } },
		});
		await vm.createContext("refreshed-timer");
		const first = await vm.javascript.evaluate(
			`new Promise(resolve=>{const timer=setTimeout(()=>{timer.refresh();resolve('first');},1);})`,
			{ contextId: "refreshed-timer", timeoutMs: 5000 },
		);
		expect(first.outcome).toBe("succeeded");
		expect(first.value).toBe("first");
		const pending = await vm.javascript.evaluate(`_getPendingTimerCount()`, {
			contextId: "refreshed-timer",
			timeoutMs: 5000,
		});
		expect(pending.outcome).toBe("succeeded");
		expect(pending.value).toBe(0);
		const second = await vm.javascript.evaluate(
			`new Promise(resolve=>setTimeout(()=>resolve(42),5))`,
			{ contextId: "refreshed-timer", timeoutMs: 5000 },
		);
		expect(second.outcome).toBe("succeeded");
		expect(second.value).toBe(42);
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}, 15000);
