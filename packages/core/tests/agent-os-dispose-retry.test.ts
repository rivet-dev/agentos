import { describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";

describe("AgentOs disposal retry", () => {
	test("retains host routes and the VM lease until remote disposal succeeds", async () => {
		const releaseEvents = vi.fn();
		const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
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
			_sessions: Map<string, unknown>;
			_sendAcpRequest(): Promise<never>;
			_sidecarLease: { dispose(): Promise<void> } | null;
		};
		const rejectPendingPermission = vi.fn();
		const cleanupTimer = setTimeout(() => {}, 60_000);
		internals._cronManager = { dispose: cronDispose };
		internals._processes.set(42, { state: "exited", exitCode: 0 });
		internals._sessions.set("session-1", {
			pendingPermissionReplies: new Map([
				[
					"permission-1",
					{
						resolve: vi.fn(),
						reject: rejectPendingPermission,
						cleanupTimer,
					},
				],
			]),
		});
		internals._sendAcpRequest = async () => {
			throw new Error("ACP close failed");
		};
		internals._sidecarLease = lease;

		try {
			await expect(vm.dispose()).rejects.toThrow("close session failed");
			expect(internals._sidecarLease).toBe(lease);
			expect(internals._processes.size).toBe(1);
			expect(internals._sessions.has("session-1")).toBe(true);
			expect(rejectPendingPermission).not.toHaveBeenCalled();
			expect(releaseEvents).not.toHaveBeenCalled();
			expect(cronDispose).not.toHaveBeenCalled();
			expect(kernelDispose).not.toHaveBeenCalled();

			await expect(vm.dispose()).resolves.toBeUndefined();
			expect(leaseDispose).toHaveBeenCalledTimes(2);
			expect(internals._sidecarLease).toBeNull();
			expect(internals._processes.size).toBe(0);
			expect(internals._sessions.size).toBe(0);
			expect(rejectPendingPermission).toHaveBeenCalledOnce();
			expect(releaseEvents).toHaveBeenCalledOnce();
			expect(cronDispose).toHaveBeenCalledOnce();
			expect(kernelDispose).not.toHaveBeenCalled();
		} finally {
			clearTimeout(cleanupTimer);
			warn.mockRestore();
		}
	});
});
