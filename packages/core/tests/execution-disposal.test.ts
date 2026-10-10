import { ExecutionOutcome } from "@rivet-dev/agentos-runtime-core/protocol";
import type { LiveEventFrame } from "@rivet-dev/agentos-runtime-core/protocol-frames";
import { SIDECAR_PROTOCOL_SCHEMA } from "@rivet-dev/agentos-runtime-core/protocol-schema";
import { SidecarRejectedError } from "@rivet-dev/agentos-runtime-core/sidecar-errors";
import { describe, expect, it, vi } from "vitest";
import { AgentOs, AgentOsExecutionWaitLimit } from "../src/agent-os.js";
import type { SidecarProcess } from "../src/sidecar/rpc-client.js";

type Response = Awaited<ReturnType<SidecarProcess["sendVmRequest"]>>;
function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (error: Error) => void;
	const promise = new Promise<T>((yes, no) => {
		resolve = yes;
		reject = no;
	});
	return { promise, resolve, reject };
}
function executionTransport() {
	const listeners = new Set<(event: LiveEventFrame) => void>();
	const requests: Array<{
		vmId: string;
		payload: Parameters<SidecarProcess["sendVmRequest"]>[2];
		response: ReturnType<typeof deferred<Response>>;
	}> = [];
	const client = {
		onEvent(handler: (event: LiveEventFrame) => void) {
			listeners.add(handler);
			return () => {
				listeners.delete(handler);
			};
		},
		sendVmRequest: vi.fn((_session, vm, payload) => {
			const response = deferred<Response>();
			requests.push({ vmId: vm.vmId, payload, response });
			return response.promise;
		}),
	};
	const complete = (vmId: string) => {
		const event: LiveEventFrame = {
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership: {
				scope: "vm",
				connection_id: "connection",
				session_id: "session",
				vm_id: vmId,
			},
			payload: {
				type: "execution_completed",
				event: {
					executionId: "operation-1",
					generation: 1n,
					outcome: ExecutionOutcome.Succeeded,
					exitCode: 0,
					error: null,
				},
			},
		};
		for (const handler of listeners) handler(event);
	};
	const makeVm = (
		vmId: string,
		dispose = async () => {},
		maxPendingExecutionWaits?: number,
	) => {
		// Match the existing Core unit fixtures: run field initializers and public
		// namespaces with a controlled transport, without starting guest code.
		const vm = Reflect.construct(AgentOs, [
			{ dispose },
			{},
			[],
			[],
			{},
			{},
			client,
			{ connectionId: "connection", sessionId: "session" },
			{ vmId },
			undefined,
			maxPendingExecutionWaits,
		]) as AgentOs;
		Object.assign(vm, { _cronManager: { dispose() {} } });
		return vm;
	};
	return { client, requests, complete, makeVm };
}
const accepted: Response = {
	type: "execution_accepted",
	response: { operationId: "operation-1", execution: null },
};
const succeeded: Response = {
	type: "execution_completed",
	response: {
		execution: null,
		outcome: ExecutionOutcome.Succeeded,
		exitCode: 0,
		error: null,
		stdout: null,
		stderr: null,
		stdoutTruncated: null,
		stderrTruncated: null,
		evaluationValue: null,
		typeScriptCheckResult: null,
	},
};
function busy(code = "execution_busy") {
	return new SidecarRejectedError(2, {
		code,
		message: "execution is busy",
		limit_name: null,
		configured_limit: null,
		current_usage: null,
		requested: null,
		unit: null,
		scope: null,
		vm_id: null,
		session_generation: null,
		capability_id: null,
		operation: null,
		configuration_path: null,
		retryable: true,
		errno: null,
	});
}
async function nextTurn() {
	await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
function observe<T>(promise: Promise<T>) {
	let outcome: { value: T } | { error: unknown } | undefined;
	const settled = promise.then(
		(value) => {
			outcome = { value };
		},
		(error) => {
			outcome = { error };
		},
	);
	return { outcome: () => outcome, settled };
}

describe("execution waits during VM disposal", () => {
	it.each([
		"admission",
		"completion",
		"result",
		"busy completion",
		"busy retry",
	])("rejects a pending %s wait before native teardown finishes", async (stage) => {
		const transport = executionTransport();
		const teardown = deferred<void>();
		const vm = transport.makeVm("a", () => teardown.promise);
		const execution = observe(vm.process.exec("pending"));
		if (stage !== "admission") {
			transport.requests[0].response.resolve(accepted);
			await nextTurn();
		}
		if (["result", "busy completion", "busy retry"].includes(stage)) {
			transport.complete("a");
			await expect.poll(() => transport.requests.length).toBe(2);
		}
		if (stage.startsWith("busy")) {
			transport.requests[1].response.reject(busy());
			await nextTurn();
			if (stage === "busy retry") {
				transport.complete("a");
				await expect.poll(() => transport.requests.length).toBe(3);
			}
		}
		const disposal = vm.dispose();
		try {
			await expect
				.poll(execution.outcome)
				.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
			// A response that arrives after disposal cannot restart result polling.
			if (stage === "admission") {
				transport.requests[0].response.resolve(accepted);
				await nextTurn();
				expect(transport.requests).toHaveLength(1);
			}
		} finally {
			teardown.resolve();
			await disposal;
		}
	});

	it("bounds pending execution waits and warns before reaching the configured limit", async () => {
		const transport = executionTransport();
		const vm = transport.makeVm("a", async () => {}, 2);
		const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
		try {
			const first = observe(vm.process.exec("first"));
			const second = observe(vm.process.exec("second"));
			await expect(vm.process.exec("over limit")).rejects.toBeInstanceOf(
				AgentOsExecutionWaitLimit,
			);
			expect(transport.requests).toHaveLength(2);
			expect(warning).toHaveBeenCalledOnce();
			expect(warning.mock.calls[0][0]).toContain("maxPendingExecutionWaits");
			await vm.dispose();
			await expect
				.poll(first.outcome)
				.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
			await expect
				.poll(second.outcome)
				.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
		} finally {
			warning.mockRestore();
			await vm.dispose();
		}
	});

	it("rejects a background process.wait result request during disposal", async () => {
		const transport = executionTransport();
		const vm = transport.makeVm("a");
		const spawning = vm.javascript.spawn("pending");
		const descriptor = {
			executionId: "operation-1",
			pid: 123,
			createdAtMs: 0n,
		};
		transport.requests[0].response.resolve({
			type: "execution_accepted",
			response: {
				operationId: "operation-1",
				execution: descriptor,
			},
		} as Response);
		const process = await spawning;
		const waiting = observe(vm.process.wait(process.pid));
		expect(transport.requests[1].payload.type).toBe("wait_execution");
		const disposal = vm.dispose();
		try {
			await expect
				.poll(waiting.outcome)
				.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
		} finally {
			const signal = transport.requests.find(
				(request) => request.payload.type === "signal_execution",
			);
			signal?.response.resolve({
				type: "execution_descriptor",
				response: { execution: descriptor },
			} as Response);
			transport.complete("a");
			await disposal;
		}
	});

	it("finishes disposal when a background execution exited before its signal arrived", async () => {
		const transport = executionTransport();
		const vm = transport.makeVm("a");
		const spawning = vm.javascript.spawn("pending");
		transport.requests[0].response.resolve({
			type: "execution_accepted",
			response: {
				operationId: "operation-1",
				execution: { executionId: "operation-1", pid: 123, createdAtMs: 0n },
			},
		} as Response);
		const process = await spawning;
		const waiting = observe(vm.process.wait(process.pid));
		const disposal = observe(vm.dispose());
		try {
			const signal = transport.requests.find(
				(request) => request.payload.type === "signal_execution",
			);
			expect(signal).toBeDefined();
			signal!.response.reject(busy("execution_not_running"));
			await nextTurn();
			transport.complete("a");
			await expect
				.poll(waiting.outcome)
				.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
			await expect.poll(disposal.outcome).toEqual({ value: undefined });
		} finally {
			transport.complete("a");
			await disposal.settled;
		}
	});

	it("does not admit an execution after disposal", async () => {
		const { client, makeVm } = executionTransport();
		const vm = makeVm("a");
		await vm.dispose();
		const execution = observe(vm.javascript.execute("42"));
		await expect
			.poll(execution.outcome)
			.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
		expect(client.sendVmRequest).not.toHaveBeenCalled();
	});

	it("preserves the native teardown error while rejecting pending calls", async () => {
		const transport = executionTransport();
		const error = new Error("native teardown failed");
		const vm = transport.makeVm("a", async () => {
			throw error;
		});
		const execution = observe(vm.process.exec("pending"));
		await expect(vm.dispose()).rejects.toBe(error);
		await expect
			.poll(execution.outcome)
			.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
	});

	it("keeps sibling executions and completed results usable on the same transport", async () => {
		const transport = executionTransport();
		const a = transport.makeVm("a");
		const b = transport.makeVm("b");
		const first = observe(a.process.exec("pending"));
		const second = b.process.exec("complete");
		transport.requests[1].response.resolve(accepted);
		await nextTurn();
		await a.dispose();
		await expect
			.poll(first.outcome)
			.toEqual({ error: new Error("ERR_AGENTOS_VM_DISPOSED") });
		transport.complete("b");
		await expect.poll(() => transport.requests.length).toBe(3);
		expect(transport.requests[2].vmId).toBe("b");
		transport.requests[2].response.resolve(succeeded);
		await expect(second).resolves.toMatchObject({
			outcome: "succeeded",
			exitCode: 0,
		});
		await b.dispose();
	});
});
