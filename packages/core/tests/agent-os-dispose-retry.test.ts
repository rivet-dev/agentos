import { describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("AgentOs disposal retry", () => {
	test("retains host routes and the VM lease until remote disposal succeeds", async () => {
		const releaseEvents = vi.fn();
		const kernelDispose = vi.fn(async () => {});
		const vm = new (AgentOs as unknown as new (
			...args: unknown[]
		) => AgentOs)(
			{ dispose: kernelDispose },
			{},
			{},
			{},
			{ onEvent: () => releaseEvents },
			{},
			{ vmId: "vm-1", processRouteRetention: 1_024 },
		);
		const cronDispose = vi.fn();
		const leaseDispose = vi
			.fn<() => Promise<void>>()
			.mockRejectedValueOnce(new Error("close session failed"))
			.mockResolvedValueOnce();
		const lease = { dispose: leaseDispose };
		const internals = vm as unknown as {
			_cronManager: { dispose(): void };
			_processes: Map<number, unknown>;
			_sidecarLease: { dispose(): Promise<void> } | null;
		};
		internals._cronManager = { dispose: cronDispose };
		internals._processes.set(42, { state: "exited", exitCode: 0 });
		internals._sidecarLease = lease;

		await expect(vm.dispose()).rejects.toThrow("close session failed");
		expect(internals._sidecarLease).toBe(lease);
		expect(internals._processes.size).toBe(1);
		expect(releaseEvents).not.toHaveBeenCalled();
		expect(cronDispose).not.toHaveBeenCalled();
		expect(kernelDispose).not.toHaveBeenCalled();

		await expect(vm.dispose()).resolves.toBeUndefined();
		expect(leaseDispose).toHaveBeenCalledTimes(2);
		expect(internals._sidecarLease).toBeNull();
		expect(internals._processes.size).toBe(0);
		expect(releaseEvents).toHaveBeenCalledOnce();
		expect(cronDispose).toHaveBeenCalledOnce();
		expect(kernelDispose).not.toHaveBeenCalled();
	});
});
