import { afterEach, expect, it, vi } from "vitest";
import { NativeSidecarKernelProxy } from "../src/kernel-proxy.js";
import { SidecarRejectedError } from "../src/sidecar-errors.js";

afterEach(() => {
	vi.useRealTimers();
	vi.restoreAllMocks();
});

function fixture() {
	const client = {
		disposeVm: vi.fn(async () => {}),
		dispose: vi.fn(async () => {}),
		execute: vi.fn(async () => ({ pid: 42 })),
		getProcessSnapshot: vi.fn(async () => []),
		getSignalState: vi.fn(async () => ({ handlers: new Map() })),
		killProcess: vi.fn(async () => {}),
		closeStdin: vi.fn(async () => {}),
		waitForEvent(_filter: unknown, _unused: unknown, options: { signal: AbortSignal }) {
			return new Promise<never>((_resolve, reject) => {
				options.signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
			});
		},
	};
	const onDispose = vi.fn(async () => {});
	const proxy = new NativeSidecarKernelProxy({
		client,
		session: { connectionId: "conn-1", sessionId: "sess-1" },
		vm: { vmId: "vm-test" },
		env: {},
		cwd: "/work",
		localMounts: [],
		commandGuestPaths: new Map(),
		onDispose,
	} as unknown as ConstructorParameters<typeof NativeSidecarKernelProxy>[0]);
	return { proxy, client, onDispose };
}

it("preserves a typed disposal rejection through secondary cleanup without fabricating exit", async () => {
	vi.spyOn(console, "error").mockImplementation(() => {});
	const { proxy, client, onDispose } = fixture();
	const rejection = new SidecarRejectedError(1, {
		code: "timeout", message: "SQLite close is unconfirmed",
		limit_name: "reactor.shutdownDeadlineMs", configured_limit: 5_000,
		current_usage: null, requested: null, unit: "milliseconds", scope: "vm",
		vm_id: "vm-test", session_generation: null, capability_id: null,
		operation: "vm.dispose", configuration_path: "limits.reactor.shutdownDeadlineMs",
		retryable: false, errno: "ETIMEDOUT",
	});
	const proc = proxy.spawn("node", []);
	proc.closeStdin();
	await vi.waitFor(() => expect(client.closeStdin).toHaveBeenCalled());
	client.disposeVm.mockRejectedValue(rejection);
	client.dispose.mockRejectedValue(new Error("secondary sidecar cleanup"));
	onDispose.mockRejectedValue(new Error("secondary callback cleanup"));
	await expect(proxy.dispose()).rejects.toBe(rejection);
	await expect(proc.wait()).rejects.toBe(rejection);
	expect(proc.exitCode).toBeNull();
	expect(client.dispose).toHaveBeenCalledOnce();
	expect(onDispose).toHaveBeenCalledOnce();
	await expect(proxy.dispose()).resolves.toBeUndefined();
	expect(client.disposeVm).toHaveBeenCalledOnce();
});

it("rejects explicitly at the existing one-second cap and logs a late failure", async () => {
	vi.useFakeTimers();
	const log = vi.spyOn(console, "error").mockImplementation(() => {});
	const { proxy, client, onDispose } = fixture();
	let rejectClose!: (error: Error) => void;
	client.disposeVm.mockImplementation(() => new Promise<void>((_resolve, reject) => { rejectClose = reject; }));
	const disposal = proxy.dispose();
	const assertion = expect(disposal).rejects.toMatchObject({
		code: "timeout", operation: "vm.dispose", vmId: "vm-test", deadlineMs: 1000,
		message: expect.stringContaining("was not confirmed"),
	});
	await vi.advanceTimersByTimeAsync(1000);
	await assertion;
	expect(client.dispose).toHaveBeenCalledOnce();
	expect(onDispose).toHaveBeenCalledOnce();
	expect(vi.getTimerCount()).toBe(0);
	const lateError = new Error("late close failure");
	rejectClose(lateError);
	await vi.advanceTimersByTimeAsync(0);
	expect(log).toHaveBeenCalledWith("agentOS VM disposal failed after its client deadline:", lateError);
});

it("clears the cap timer after confirmed disposal", async () => {
	vi.useFakeTimers();
	const { proxy } = fixture();
	await proxy.dispose();
	expect(vi.getTimerCount()).toBe(0);
});
