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

describe("sidecar-authoritative command resolution", () => {
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
		const execute = vi.fn(async () => {
			processId = "sidecar-process-1";
			return { processId, pid: 42 };
		});
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
			execute,
			closeStdin: vi.fn(async () => {}),
			disposeVm: vi.fn(async () => {
				stopped = true;
			}),
			dispose: vi.fn(async () => {
				stopped = true;
			}),
		} as unknown as NativeSidecarProcessClient;

		return { client, execute };
	}

	test("forwards commands without a client-side command registry", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-wasm-tiers-"));
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

		const proc = await proxy.spawn("grep", ["needle", "haystack.txt"], {
			cwd: "/workspace",
		});
		const exitCode = await proc.wait();

		expect(exitCode).toBe(0);
		expect(proxy).not.toHaveProperty("commands");
		expect(proxy).not.toHaveProperty("registerCommandGuestPaths");
		expect(execute).toHaveBeenCalledTimes(1);
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("processId");
		expect(execute.mock.calls[0]?.[2]).toMatchObject({
			command: "grep",
			args: ["needle", "haystack.txt"],
			cwd: "/workspace",
		});
	});

	test("exec forwards the raw command line to the sidecar", async () => {
		fixtureRoot = mkdtempSync(join(tmpdir(), "agentos-wasm-tiers-"));
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
			proxy.exec("echo changed >> /tmp/write-only.txt"),
		).resolves.toMatchObject({ exitCode: 0 });
		expect(execute.mock.calls[0]?.[2]).toMatchObject({
			shellCommand: "echo changed >> /tmp/write-only.txt",
			args: [],
		});
		expect(execute.mock.calls[0]?.[2]).not.toHaveProperty("command");
	});
});
