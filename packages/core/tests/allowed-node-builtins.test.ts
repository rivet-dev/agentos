import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, test, vi } from "vitest";
import type {
	AuthenticatedSession,
	CreatedVm,
	NativeSidecarProcessClient,
} from "../src/sidecar/rpc-client.js";
import { NativeSidecarKernelProxy } from "../src/sidecar/rpc-client.js";

describe("NativeSidecarKernelProxy execute payloads", () => {
	let proxy: NativeSidecarKernelProxy | null = null;
	let fixtureRoot: string | null = null;

	afterEach(async () => {
		await proxy?.dispose();
		proxy = null;
		if (fixtureRoot) {
			rmSync(fixtureRoot, { recursive: true, force: true });
			fixtureRoot = null;
		}
	});

	function createMockClient() {
		let stopped = false;
		let processId: string | null = null;
		let exitDelivered = false;
		const execute = vi.fn(
			async (
				_session: AuthenticatedSession,
				_vm: CreatedVm,
				_execution: { env?: Record<string, string> },
			) => {
				processId = "sidecar-process-1";
				return { processId, pid: 42 };
			},
		);
		const client = {
			waitForEvent: vi.fn(async () => {
				while (!stopped) {
					if (processId !== null && !exitDelivered) {
						exitDelivered = true;
						return {
							ownership: { scope: "vm", vm_id: "vm-1" },
							payload: {
								type: "process_exited",
								process_id: processId,
								exit_code: 0,
							},
						};
					}
					await new Promise((resolve) => setTimeout(resolve, 1));
				}
				throw new Error("mock stopped");
			}),
			writeStdin: vi.fn(async () => {}),
			closeStdin: vi.fn(async () => {}),
			execute,
			disposeVm: vi.fn(async () => {
				stopped = true;
			}),
			dispose: vi.fn(async () => {
				stopped = true;
			}),
		} as unknown as NativeSidecarProcessClient;

		return { client, execute };
	}

	async function captureExecutePayload() {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-allowed-builtins-"));
		const { client, execute } = createMockClient();

		proxy = new NativeSidecarKernelProxy({
			client,
			session: {
				connectionId: "conn-1",
				sessionId: "session-1",
			} as AuthenticatedSession,
			vm: { vmId: "vm-1" } as CreatedVm,
			env: { HOME: "/workspace" },
			cwd: "/workspace",
			localMounts: [],
			sidecarMounts: [],
		});

		const proc = await proxy.spawn("node", ["/workspace/entry.mjs"], {
			cwd: "/workspace",
			env: { HOME: "/workspace" },
		});
		const exitCode = await proc.wait();

		expect(exitCode).toBe(0);
		expect(execute).toHaveBeenCalledTimes(1);
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("processId");
		return execute.mock.calls[0]?.[2];
	}

	test("leaves internal AGENT_OS runtime env construction to the sidecar", async () => {
		await expect(captureExecutePayload()).resolves.toMatchObject({
			command: "node",
			args: ["/workspace/entry.mjs"],
			cwd: "/workspace",
			env: { HOME: "/workspace" },
		});
		await expect(captureExecutePayload()).resolves.not.toMatchObject({
			env: {
				AGENT_OS_ALLOWED_NODE_BUILTINS: expect.anything(),
			},
		});
	});

	test("exec omits sidecar-owned cwd and env defaults", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-shell-exec-"));
		const { client, execute } = createMockClient();

		proxy = new NativeSidecarKernelProxy({
			client,
			session: {
				connectionId: "conn-1",
				sessionId: "session-1",
			} as AuthenticatedSession,
			vm: { vmId: "vm-1" } as CreatedVm,
			env: { HOME: "/workspace" },
			cwd: "/workspace",
			localMounts: [],
			sidecarMounts: [],
		});

		await expect(
			proxy.exec("node /workspace/entry.mjs --flag"),
		).resolves.toMatchObject({
			exitCode: 0,
		});
		expect(execute).toHaveBeenCalledTimes(1);
		expect(execute.mock.calls[0]?.[2]).toMatchObject({
			shellCommand: "node /workspace/entry.mjs --flag",
			args: [],
		});
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("command");
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("cwd");
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("env");
	});

	test("openShell sends only explicit PTY options and leaves defaults to the sidecar", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-shell-pty-"));
		const { client, execute } = createMockClient();

		proxy = new NativeSidecarKernelProxy({
			client,
			session: {
				connectionId: "conn-1",
				sessionId: "session-1",
			} as AuthenticatedSession,
			vm: { vmId: "vm-1" } as CreatedVm,
			env: { HOME: "/home/agentos" },
			cwd: "/workspace",
			localMounts: [],
			sidecarMounts: [],
		});

		const shell = await proxy.openShell({ cols: 100, rows: 40 });
		await expect(shell.wait()).resolves.toBe(0);

		const payload = execute.mock.calls[0]?.[2] as
			| {
					env?: Record<string, string>;
					pty?: { cols?: number; rows?: number };
					keepStdinOpen?: boolean;
			  }
			| undefined;
		expect(payload?.pty).toEqual({ cols: 100, rows: 40 });
		expect(payload?.keepStdinOpen).toBeUndefined();
		expect(payload?.env).toBeUndefined();
		expect(execute.mock.calls[0]?.[2]).toMatchObject({ args: [] });
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("command");
	});

	test("spawn preserves false, true, and omission for streamStdin", async () => {
		const cases = [
			{ label: "false", value: false, expected: false },
			{ label: "true", value: true, expected: true },
			{ label: "omitted", value: undefined, expected: undefined },
		] as const;

		for (const testCase of cases) {
			const { client, execute } = createMockClient();
			proxy = new NativeSidecarKernelProxy({
				client,
				session: {
					connectionId: "conn-1",
					sessionId: "session-1",
				} as AuthenticatedSession,
				vm: { vmId: "vm-1" } as CreatedVm,
				env: { HOME: "/workspace" },
				cwd: "/workspace",
				localMounts: [],
				sidecarMounts: [],
			});

			const options =
				testCase.value === undefined
					? undefined
					: { streamStdin: testCase.value };
			const process = await proxy.spawn("node", [`${testCase.label}.mjs`], options);
			await expect(process.wait()).resolves.toBe(0);
			const payload = execute.mock.calls[0]?.[2];
			if (testCase.expected === undefined) {
				expect(payload).not.toHaveProperty("keepStdinOpen");
			} else {
				expect(payload).toHaveProperty(
					"keepStdinOpen",
					testCase.expected,
				);
			}

			await proxy.dispose();
			proxy = null;
		}
	});
});
