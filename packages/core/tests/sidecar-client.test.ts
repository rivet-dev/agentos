import { describe, expect, it } from "vitest";
import {
	SidecarProcessExited,
	SidecarRejectedError,
} from "../src/sidecar-errors.js";
import {
	AgentOsSidecarClient,
	type AgentOsSidecarSessionBootstrap,
	type AgentOsSidecarVmBootstrap,
} from "../src/sidecar/rpc-client.js";

describe("AgentOsSidecarClient", () => {
	it("tracks sidecar session and VM lifecycle through a mock transport", async () => {
		const calls: Array<
			| { type: "session"; bootstrap: AgentOsSidecarSessionBootstrap }
			| { type: "vm"; bootstrap: AgentOsSidecarVmBootstrap }
			| { type: "dispose-vm"; vmId: string }
			| { type: "dispose-session" }
		> = [];
		let tick = 100;
		let nextId = 0;
		const client = new AgentOsSidecarClient({
			createId: () => `id-${++nextId}`,
			now: () => ++tick,
			async createOwnershipTransport(bootstrap) {
				calls.push({ type: "session", bootstrap });
				return {
					async createVm(vmBootstrap) {
						calls.push({ type: "vm", bootstrap: vmBootstrap });
					},
					async disposeVm(vmId) {
						calls.push({ type: "dispose-vm", vmId });
					},
					async dispose() {
						calls.push({ type: "dispose-session" });
					},
				};
			},
		});

		const session = await client.createOwnershipSession({
			placement: { kind: "shared", pool: "default" },
			metadata: { owner: "core-test" },
		});
		expect(session.describe()).toMatchObject({
			sessionId: "id-1",
			state: "ready",
			placement: { kind: "shared", pool: "default" },
			metadata: { owner: "core-test" },
			vmIds: [],
		});

		const vm = await session.createVm({
			metadata: { runtime: "javascript" },
		});
		expect(vm.describe()).toMatchObject({
			vmId: "id-2",
			sessionId: "id-1",
			state: "ready",
			metadata: { runtime: "javascript" },
		});
		expect(session.listVms()).toEqual([vm.describe()]);
		expect(client.listSessions()).toEqual([session.describe()]);

		await vm.dispose();
		expect(vm.describe()).toMatchObject({
			vmId: "id-2",
			state: "disposed",
		});

		await session.dispose();
		expect(session.describe()).toMatchObject({
			sessionId: "id-1",
			state: "disposed",
		});

		expect(calls).toEqual([
			{
				type: "session",
				bootstrap: {
					sessionId: "id-1",
					placement: { kind: "shared", pool: "default" },
					metadata: { owner: "core-test" },
					signal: undefined,
				},
			},
			{
				type: "vm",
				bootstrap: {
					vmId: "id-2",
					sessionId: "id-1",
					metadata: { runtime: "javascript" },
				},
			},
			{
				type: "dispose-vm",
				vmId: "id-2",
			},
			{
				type: "dispose-session",
			},
		]);
	});

	it("disposes every tracked session when the client is torn down", async () => {
		const disposedSessions: string[] = [];
		const disposedVms: string[] = [];
		let nextId = 0;
		const client = new AgentOsSidecarClient({
			createId: () => `id-${++nextId}`,
			async createOwnershipTransport(bootstrap) {
				return {
					async createVm(vmBootstrap) {
						disposedSessions.push(
							`create:${bootstrap.sessionId}:${vmBootstrap.vmId}`,
						);
					},
					async disposeVm(vmId) {
						disposedVms.push(vmId);
					},
					async dispose() {
						disposedSessions.push(`dispose:${bootstrap.sessionId}`);
					},
				};
			},
		});

		const first = await client.createOwnershipSession();
		const second = await client.createOwnershipSession({
			placement: { kind: "explicit", sidecarId: "shared-sidecar-2" },
		});
		const vm = await second.createVm();

		await client.dispose();

		expect(first.describe().state).toBe("disposed");
		expect(second.describe().state).toBe("disposed");
		expect(vm.describe().state).toBe("disposed");
		expect(disposedVms).toEqual(["id-3"]);
		expect(disposedSessions).toEqual([
			"create:id-2:id-3",
			"dispose:id-1",
			"dispose:id-2",
		]);
	});
});

async function rejectedReason(promise: Promise<void>): Promise<unknown> {
	const [result] = await Promise.allSettled([promise]);
	expect(result.status).toBe("rejected");
	if (result.status !== "rejected")
		throw new Error("expected disposal rejection");
	return result.reason;
}

function structuredDisposalError() {
	return new SidecarRejectedError(7, {
		code: "timeout",
		message: "injected unconfirmed VM disposal",
		limit_name: "reactor.shutdownDeadlineMs",
		configured_limit: 5000,
		current_usage: null,
		requested: null,
		unit: "milliseconds",
		scope: "vm",
		vm_id: "injected-vm",
		session_generation: null,
		capability_id: null,
		operation: "vm.dispose",
		configuration_path: "limits.reactor.shutdownDeadlineMs",
		retryable: false,
		errno: "ETIMEDOUT",
	});
}

describe("sidecar disposal preserves original terminal errors", () => {
	it.each([
		new SidecarProcessExited({
			exitCode: 17,
			signal: null,
			stderr: "injected bounded stderr",
		}),
		structuredDisposalError(),
		new Error("injected unexpected VM cleanup failure"),
	])("retains sole %s through VM, session, client and repeated disposal", async (failure) => {
		const calls: string[] = [];
		const client = new AgentOsSidecarClient({
			async createOwnershipTransport() {
				return {
					async disposeVm() {
						calls.push("vm");
						throw failure;
					},
					async dispose() {
						calls.push("session");
					},
				};
			},
		});
		const session = await client.createOwnershipSession();
		const vm = await session.createVm();
		expect(await rejectedReason(client.dispose())).toBe(failure);
		expect(await rejectedReason(vm.dispose())).toBe(failure);
		expect(await rejectedReason(session.dispose())).toBe(failure);
		expect(await rejectedReason(client.dispose())).toBe(failure);
		expect(calls).toEqual(["vm", "session"]);
		expect(vm.describe()).toMatchObject({
			state: "failed",
			lastError: failure.message,
		});
		expect(session.describe()).toMatchObject({
			state: "failed",
			lastError: failure.message,
		});
	});

	it("retains a transport-only typed failure without replaying its disposal", async () => {
		const failure = new SidecarProcessExited({
			exitCode: null,
			signal: "SIGKILL",
			stderr: "",
		});
		let calls = 0;
		const client = new AgentOsSidecarClient({
			async createOwnershipTransport() {
				return {
					async dispose() {
						calls++;
						throw failure;
					},
				};
			},
		});
		const session = await client.createOwnershipSession();
		expect(await rejectedReason(session.dispose())).toBe(failure);
		expect(await rejectedReason(client.dispose())).toBe(failure);
		expect(await rejectedReason(session.dispose())).toBe(failure);
		expect(calls).toBe(1);
	});

	it("retains every VM and transport failure while still attempting all cleanup", async () => {
		const vmFailure = structuredDisposalError();
		const transportFailure = new Error("injected transport shutdown failure");
		const calls: string[] = [];
		const client = new AgentOsSidecarClient({
			async createOwnershipTransport() {
				return {
					async disposeVm() {
						calls.push("vm");
						throw vmFailure;
					},
					async dispose() {
						calls.push("session");
						throw transportFailure;
					},
				};
			},
		});
		const session = await client.createOwnershipSession();
		const vm = await session.createVm();
		const result = await rejectedReason(client.dispose());
		expect(result).toBeInstanceOf(AggregateError);
		if (!(result instanceof AggregateError))
			throw new Error("expected all cleanup errors");
		expect(result.errors).toHaveLength(2);
		expect(result.errors[0]).toBe(vmFailure);
		expect(result.errors[1]).toBe(transportFailure);
		expect(result.message).toBe(
			`${vmFailure.message}; ${transportFailure.message}`,
		);
		expect(await rejectedReason(session.dispose())).toBe(result);
		expect(await rejectedReason(client.dispose())).toBe(result);
		expect(await rejectedReason(vm.dispose())).toBe(vmFailure);
		expect(calls).toEqual(["vm", "session"]);
		expect(session.describe()).toMatchObject({
			state: "failed",
			lastError: result.message,
		});
	});

	it("retains separate session errors in order without flattening or skipping later owners", async () => {
		const failures = [
			structuredDisposalError(),
			new Error("injected second session failure"),
		];
		const calls: number[] = [];
		let created = 0;
		const client = new AgentOsSidecarClient({
			async createOwnershipTransport() {
				const index = created++;
				return {
					async dispose() {
						calls.push(index);
						throw failures[index];
					},
				};
			},
		});
		const first = await client.createOwnershipSession();
		const second = await client.createOwnershipSession();
		const result = await rejectedReason(client.dispose());
		expect(result).toBeInstanceOf(AggregateError);
		if (!(result instanceof AggregateError))
			throw new Error("expected both session errors");
		expect(result.errors).toHaveLength(2);
		expect(result.errors[0]).toBe(failures[0]);
		expect(result.errors[1]).toBe(failures[1]);
		expect(await rejectedReason(first.dispose())).toBe(failures[0]);
		expect(await rejectedReason(second.dispose())).toBe(failures[1]);
		expect(await rejectedReason(client.dispose())).toBe(result);
		expect(calls).toEqual([0, 1]);
	});

	it("concurrent disposal shares one pending attempt and the same original failure", async () => {
		const failure = new Error("injected gated cleanup failure");
		let release!: () => void;
		let announce!: () => void;
		const gate = new Promise<void>((resolve) => {
			release = resolve;
		});
		const entered = new Promise<void>((resolve) => {
			announce = resolve;
		});
		let calls = 0;
		const client = new AgentOsSidecarClient({
			async createOwnershipTransport() {
				return {
					async dispose() {
						calls++;
						announce();
						await gate;
						throw failure;
					},
				};
			},
		});
		await client.createOwnershipSession();
		const first = client.dispose();
		const second = client.dispose();
		const outcomes = Promise.allSettled([first, second]);
		try {
			expect(
				await Promise.race([
					entered.then(() => "entered"),
					outcomes.then(() => "settled-too-early"),
				]),
			).toBe("entered");
			expect(first).toBe(second);
			expect(calls).toBe(1);
		} finally {
			release();
		}
		for (const result of await outcomes) {
			expect(result.status).toBe("rejected");
			if (result.status === "rejected") expect(result.reason).toBe(failure);
		}
		expect(await rejectedReason(client.dispose())).toBe(failure);
		expect(calls).toBe(1);
	});
});
