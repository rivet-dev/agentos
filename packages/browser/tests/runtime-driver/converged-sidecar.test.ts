import { describe, expect, it } from "vitest";
import { createConvergedExecutionHostBridge } from "../../src/converged-execution-host-bridge.js";
import { createAgentOsConvergedSidecar } from "../../src/converged-sidecar.js";

// Unit coverage for the Agent OS converged-consumption layer (the TS that plugs
// the ACP wasm sidecar into @rivet-dev/agentos-runtime-browser's converged runtime). The
// end-to-end Chromium proof lives in tests/browser-wasm/converged-runtime.spec.ts;
// these are the fast headless units that pin the contract.

describe("converged execution host bridge", () => {
	it("echoes the execution id startExecution returns", () => {
		const host = createConvergedExecutionHostBridge();
		// Default before any set.
		expect(host.bridge.startExecution("{}")).toEqual({
			executionId: "converged-exec",
		});
		host.setNextExecutionId("exec-42");
		expect(host.bridge.startExecution("{}")).toEqual({
			executionId: "exec-42",
		});
	});

	it("mints distinct javascript context ids", () => {
		const host = createConvergedExecutionHostBridge();
		const a = host.bridge.createJavascriptContext("{}") as {
			contextId: string;
		};
		const b = host.bridge.createJavascriptContext("{}") as {
			contextId: string;
		};
		expect(a.contextId).not.toBe(b.contextId);
		expect(a.contextId).toMatch(/^converged-ctx-/);
	});

	it("treats the execution lifecycle callbacks as no-ops", () => {
		const host = createConvergedExecutionHostBridge();
		expect(host.bridge.writeExecutionStdin("{}")).toEqual({});
		expect(host.bridge.closeExecutionStdin("{}")).toEqual({});
		expect(host.bridge.killExecution("{}")).toEqual({});
		// No buffered events: the converged driver surfaces them on its own channels.
		expect(host.bridge.pollExecutionEvent("{}")).toBeNull();
	});

	it("reflects the worker runtime in createWorker", () => {
		const host = createConvergedExecutionHostBridge();
		const worker = host.bridge.createWorker(
			JSON.stringify({ runtime: "java_script" }),
		) as { workerId: string; runtime?: string };
		expect(worker.runtime).toBe("java_script");
		expect(worker.workerId).toMatch(/^converged-worker-/);
	});

	it("keeps ACP process handles bound to their exact agent executions", () => {
		let agentNumber = 0;
		const host = createConvergedExecutionHostBridge({
			agentExecutor: {
				createAgent() {
					agentNumber += 1;
					const current = agentNumber;
					return { handleLine: (line) => [`agent-${current}:${line}`] };
				},
			},
		});
		const first = host.bridge.startExecution(
			JSON.stringify({ vmId: "vm-1", argv: ["echo"], cwd: "/" }),
		) as { executionId: string };
		host.bindPendingProcess("acp-agent-1");
		const second = host.bridge.startExecution(
			JSON.stringify({ vmId: "vm-1", argv: ["echo"], cwd: "/" }),
		) as { executionId: string };
		host.bindPendingProcess("acp-agent-2");

		const encode = (text: string) =>
			btoa(String.fromCharCode(...new TextEncoder().encode(text)));
		host.bridge.writeExecutionStdin(
			JSON.stringify({
				executionId: first.executionId,
				chunkBase64: encode("first prompt\n"),
			}),
		);
		host.bridge.writeExecutionStdin(
			JSON.stringify({
				executionId: second.executionId,
				chunkBase64: encode("resume prompt\n"),
			}),
		);

		const firstOutput = host.pollAgentOutput("acp-agent-1", Date.now() + 1_000);
		const secondOutput = host.pollAgentOutput(
			"acp-agent-2",
			Date.now() + 1_000,
		);
		expect(new TextDecoder().decode(firstOutput?.payload)).toBe(
			"agent-1:first prompt\n",
		);
		expect(new TextDecoder().decode(secondOutput?.payload)).toBe(
			"agent-2:resume prompt\n",
		);
	});

	it("drops sessions and process bindings when a worker terminates", () => {
		const host = createConvergedExecutionHostBridge({
			agentExecutor: {
				createAgent() {
					return { handleLine: (line) => [line] };
				},
			},
		});
		const first = host.bridge.startExecution(
			JSON.stringify({ vmId: "vm-1", argv: ["echo"], cwd: "/" }),
		) as { executionId: string };
		host.bindPendingProcess("acp-agent-1");
		host.bridge.terminateWorker(
			JSON.stringify({ executionId: first.executionId }),
		);
		expect(() => host.pollAgentOutput("acp-agent-1", Date.now())).toThrow(
			"unknown ACP process",
		);
		expect(() => host.bindPendingProcess("acp-agent-1")).toThrow(
			"no agent execution was spawned",
		);

		const second = host.bridge.startExecution(
			JSON.stringify({ vmId: "vm-1", argv: ["echo"], cwd: "/" }),
		) as { executionId: string };
		expect(second.executionId).not.toBe(first.executionId);
		host.bindPendingProcess("acp-agent-1");
	});
});

describe("createAgentOsConvergedSidecar", () => {
	const config = { permissions: { fs: "allow" } } as never;

	it("builds ConvergedSidecarFactoryOptions carrying the config + bare codec", () => {
		const options = createAgentOsConvergedSidecar(config);
		expect(options.config).toBe(config);
		expect(options.codec).toBe("bare");
		expect(typeof options.loadSidecar).toBe("function");
	});

	it("honours an explicit codec + onFsReadDenied override", () => {
		const onFsReadDenied = () => {};
		const options = createAgentOsConvergedSidecar(config, {
			codec: "json" as never,
			onFsReadDenied,
		});
		expect(options.codec).toBe("json");
		expect(options.onFsReadDenied).toBe(onFsReadDenied);
	});

	it("forwards complete package artifacts and custom roots without decoding metadata", () => {
		const content = new Uint8Array([1, 2, 3]);
		const options = createAgentOsConvergedSidecar(config, {
			packageBytes: [content],
			packagesMountAt: "/srv/agentos",
		});
		expect(options.packages).toEqual([{ content }]);
		expect(options.packagesMountAt).toBe("/srv/agentos");
	});
});
