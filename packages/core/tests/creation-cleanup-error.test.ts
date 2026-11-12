import { afterEach, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { SidecarRejectedError } from "../src/sidecar-errors.js";
import { SidecarProcess } from "../src/sidecar/process-client.js";

// Real AgentOs.create/lease/transport owners; only the native transport is injected.
// No subprocess, VM, native artifact, sleeps or real timeout is needed.
afterEach(() => {
	vi.restoreAllMocks();
	vi.unstubAllEnvs();
});

function quotaError(requestId: number, operation: string) {
	return new SidecarRejectedError(requestId, {
		code: "ERR_AGENTOS_VFS",
		message: "injected inode quota exhaustion",
		limit_name: "filesystem.maxInodes",
		configured_limit: 128,
		current_usage: 128,
		requested: 1,
		unit: "inodes",
		scope: "vm",
		vm_id: "injected-vm",
		session_generation: null,
		capability_id: null,
		operation,
		configuration_path: "limits.filesystem.maxInodes",
		retryable: false,
		errno: "ENOSPC",
	});
}

function injectTransport(
	stage: "ready" | "configure",
	primary: Error,
	disposeVm: () => Promise<void>,
) {
	// Exercise the existing published-binary resolver without Cargo or execution.
	// spawn is intercepted below before its implementation can launch anything.
	vi.stubEnv("AGENTOS_SIDECAR_BIN", process.execPath);
	const events: string[] = [];
	const client = {
		protocolClient: {
			child: { ref: vi.fn(), unref: vi.fn(), stdio: [] },
		},
		authenticateAndOpenSession: vi.fn(async () => ({
			connectionId: "injected-connection",
			sessionId: "injected-session",
		})),
		onClose: vi.fn(() => () => {}),
		createVm: vi.fn(async () => ({ vmId: "injected-vm" })),
		waitForEvent: vi.fn(async () => {
			if (stage === "ready") throw primary;
		}),
		configureVm: vi.fn(async () => {
			throw primary;
		}),
		disposeVm: vi.fn(async () => {
			events.push("delete-start");
			try {
				await disposeVm();
			} finally {
				events.push("delete-settled");
			}
		}),
		setVmSidecarRequestHandler: vi.fn(() => events.push("route-cleanup")),
		dispose: vi.fn(async () => events.push("owner-dispose")),
	};
	const spawn = vi
		.spyOn(SidecarProcess, "spawn")
		.mockReturnValue(client as unknown as SidecarProcess);
	return { client, events, spawn };
}

function create(sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>) {
	return AgentOs.create({
		defaultSoftware: false,
		sidecar: { kind: "explicit", handle: sidecar },
		// Skip unrelated directory bootstrapping; no native plugin is executed.
		rootFilesystem: { type: "native", plugin: { id: "injected-fixture" } },
	});
}

test.each([
	["ready", false],
	["ready", true],
	["configure", false],
	["configure", true],
] as const)("failed public create preserves typed %s error and reports cleanupFailure=%s", async (stage, cleanupFails) => {
	const primary = quotaError(1, "vm.configure");
	const secondary = quotaError(2, "vm.dispose");
	const logged = vi.spyOn(console, "error").mockImplementation(() => {});
	const fixture = injectTransport(stage, primary, async () => {
		if (cleanupFails) throw secondary;
	});
	const sidecar = await AgentOs.createSidecar();
	try {
		await expect(create(sidecar)).rejects.toBe(primary);
		expect(primary.detail.errno).toBe("ENOSPC");
		expect(fixture.spawn).toHaveBeenCalledTimes(1);
		expect(fixture.spawn.mock.calls[0][0]?.command).toBe(process.execPath);
		expect(fixture.client.disposeVm).toHaveBeenCalledTimes(1);
		expect(fixture.client.setVmSidecarRequestHandler).toHaveBeenCalledWith(
			"injected-vm",
			null,
		);
		expect(fixture.events).toEqual([
			"delete-start",
			"delete-settled",
			"route-cleanup",
		]);
		if (cleanupFails) {
			expect(logged).toHaveBeenCalledTimes(1);
			expect(logged).toHaveBeenCalledWith(
				"[agentOS] native VM cleanup failed after VM creation failed",
				secondary,
			);
		} else expect(logged).not.toHaveBeenCalled();
		// VM cleanup must not terminate a shared process or manufacture success.
		expect(fixture.client.dispose).not.toHaveBeenCalled();
		expect(sidecar.describe().activeVmCount).toBe(0);
	} finally {
		await sidecar.dispose();
	}
	expect(fixture.client.dispose).toHaveBeenCalledTimes(1);
	expect(fixture.client.disposeVm).toHaveBeenCalledTimes(1);
	expect(sidecar.describe().state).toBe("disposed");
});

test("failed create awaits cleanup, releases its route, and never retries deletion", async () => {
	const primary = quotaError(3, "vm.configure");
	const secondary = new Error("injected unexpected cleanup failure");
	const logged = vi.spyOn(console, "error").mockImplementation(() => {});
	let release!: () => void;
	let entered!: () => void;
	const gate = new Promise<void>((resolve) => {
		release = resolve;
	});
	const deleting = new Promise<void>((resolve) => {
		entered = resolve;
	});
	const fixture = injectTransport("configure", primary, async () => {
		entered();
		await gate;
		throw secondary;
	});
	const sidecar = await AgentOs.createSidecar();
	let settled = false;
	const pending = create(sidecar).then(
		(value) => {
			settled = true;
			return { status: "fulfilled" as const, value };
		},
		(reason: unknown) => {
			settled = true;
			return { status: "rejected" as const, reason };
		},
	);
	let bodyFailed = false;
	let bodyError: unknown;
	try {
		const first = await Promise.race([
			deleting.then(() => "delete-entered" as const),
			pending.then(() => "create-settled" as const),
		]);
		expect(
			first,
			"creation settled before entering its required deletion",
		).toBe("delete-entered");
		expect(settled).toBe(false);
		expect(fixture.client.setVmSidecarRequestHandler).not.toHaveBeenCalled();
		release();
		const outcome = await pending;
		expect(outcome.status).toBe("rejected");
		if (outcome.status === "rejected") expect(outcome.reason).toBe(primary);
		expect(logged).toHaveBeenCalledTimes(1);
		expect(logged).toHaveBeenCalledWith(
			"[agentOS] native VM cleanup failed after VM creation failed",
			secondary,
		);
		expect(fixture.events).toEqual([
			"delete-start",
			"delete-settled",
			"route-cleanup",
		]);
	} catch (error) {
		bodyFailed = true;
		bodyError = error;
	} finally {
		release();
		const errors: unknown[] = bodyFailed ? [bodyError] : [];
		try {
			const outcome = await pending;
			if (outcome.status === "fulfilled") await outcome.value.dispose();
		} catch (error) {
			errors.push(error);
		} finally {
			// Independent of unexpected success/disposal failure while draining.
			// Wait for admitted creation to settle before closing its final owner.
			try {
				await sidecar.dispose();
			} catch (error) {
				errors.push(error);
			}
		}
		if (errors.length === 1) throw errors[0];
		if (errors.length > 1)
			throw new AggregateError(
				errors,
				"creation fixture body and cleanup failed",
			);
	}
	expect(fixture.client.disposeVm).toHaveBeenCalledTimes(1);
	expect(fixture.client.dispose).toHaveBeenCalledTimes(1);
});
