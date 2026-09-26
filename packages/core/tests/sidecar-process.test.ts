import { describe, expect, test, vi } from "vitest";
import type { LiveOwnershipScope } from "../src/ownership.js";
import type {
	LiveEventFrame,
	LiveResponseFrame,
	LiveSidecarRequestHandler,
} from "../src/protocol-frames.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";
import type { LiveRequestPayload } from "../src/request-payloads.js";
import type { SidecarProcessTransport } from "../src/sidecar-client.js";
import { SidecarProcess } from "../src/sidecar-process.js";
import type { CreateVmConfig } from "../src/vm-config.js";

class MemorySidecarTransport implements SidecarProcessTransport {
	readonly requests: Array<{
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
	}> = [];
	disposed = false;
	failed: Error | null = null;
	private sidecarRequestHandler: LiveSidecarRequestHandler | null = null;
	private readonly eventListeners = new Set<(event: LiveEventFrame) => void>();

	setSidecarRequestHandler(handler: LiveSidecarRequestHandler | null): void {
		this.sidecarRequestHandler = handler;
	}

	onEvent(handler: (event: LiveEventFrame) => void): () => void {
		this.eventListeners.add(handler);
		return () => {
			this.eventListeners.delete(handler);
		};
	}

	async sendRequest(input: {
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
	}): Promise<LiveResponseFrame> {
		this.requests.push(input);
		if (input.payload.type !== "create_layer") {
			throw new Error(`unexpected request ${input.payload.type}`);
		}
		return {
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: this.requests.length,
			ownership: input.ownership,
			payload: { type: "layer_created", layer_id: "layer-from-memory" },
		};
	}

	async waitForEvent(): Promise<LiveEventFrame> {
		throw new Error("waitForEvent not implemented for this test");
	}

	failPermanently(error: Error): void {
		this.failed = error;
	}

	async dispose(): Promise<void> {
		this.disposed = true;
	}
}

describe("sidecar process transport injection", () => {
	test("acquires metadata without a VM and preserves transport rejection", async () => {
		const transport = new MemorySidecarTransport();
		const metadata = {
			package_id: "sha256:abc",
			digest: "sha256:abc",
			size: 12n,
			package_name: "tool",
			version: "1",
			commands: ["tool"],
		};
		const send = vi
			.spyOn(transport, "sendRequest")
			.mockImplementation(async (input) => ({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 1,
				ownership: input.ownership,
				payload: { type: "package_acquired", ...metadata },
			}));
		const process = SidecarProcess.fromClient(transport);
		const session = { connectionId: "conn", sessionId: "session" };
		const options = {
			source: {
				type: "url" as const,
				url: "https://packages.gameinc.io/tool.aospkg",
			},
			advisory: true,
			timeout_ms: 25n,
		};
		try {
			expect(await process.acquirePackage(session, options)).toEqual(metadata);
			expect(send).toHaveBeenCalledWith({
				ownership: {
					scope: "session",
					connection_id: "conn",
					session_id: "session",
				},
				payload: { ...options, type: "acquire_package" },
			});
			const rejection = Object.assign(new Error("package limit"), {
				detail: {
					code: "ERR_AGENTOS_RESOURCE_LIMIT",
					configured_limit: 1,
					requested: 12,
				},
			});
			send.mockRejectedValueOnce(rejection);
			await expect(process.acquirePackage(session, options)).rejects.toBe(
				rejection,
			);
		} finally {
			await process.dispose();
			vi.restoreAllMocks();
		}
	});

	test("installs packages with VM ownership and reads session cache stats", async () => {
		const transport = new MemorySidecarTransport();
		const send = vi
			.spyOn(transport, "sendRequest")
			.mockImplementation(async (input) => ({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: transport.requests.length + 1,
				ownership: input.ownership,
				payload:
					input.payload.type === "install_package"
						? {
								type: "package_installed",
								package: {
									package_id: "sha256:test",
									digest: "sha256:test",
									size: 12n,
									package_name: "tool",
									version: "1",
									commands: ["tool"],
								},
								projected_commands: [
									{ name: "tool", guest_path: "/opt/agentos/bin/tool" },
								],
							}
						: {
								type: "package_cache_stats",
								entries: 1n,
								source_entries: 1n,
								bytes: 12n,
								pinned_entries: 1n,
								pending_acquisitions: 0n,
								hits: 0n,
								misses: 1n,
								coalesced_waiters: 0n,
								acquisitions: 1n,
								evictions: 0n,
								capacity_failures: 0n,
								cancelled_acquisitions: 0n,
							},
			}));
		const process = SidecarProcess.fromClient(transport);
		const session = { connectionId: "conn", sessionId: "session" };
		try {
			const installed = await process.installPackage(
				session,
				{ vmId: "vm" },
				{
					source: {
						type: "url",
						url: "https://packages.gameinc.io/tool.aospkg",
					},
				},
			);
			expect(installed.package.package_name).toBe("tool");
			expect(send.mock.calls[0]?.[0]).toMatchObject({
				ownership: {
					scope: "vm",
					connection_id: "conn",
					session_id: "session",
					vm_id: "vm",
				},
				payload: { type: "install_package" },
			});
			expect((await process.getPackageCacheStats(session)).pinned_entries).toBe(
				1n,
			);
			expect(send.mock.calls[1]?.[0]).toMatchObject({
				ownership: {
					scope: "session",
					connection_id: "conn",
					session_id: "session",
				},
				payload: { type: "get_package_cache_stats" },
			});
		} finally {
			await process.dispose();
			vi.restoreAllMocks();
		}
	});

	test("compares creation config using session ownership without creating a VM", async () => {
		const transport = new MemorySidecarTransport();
		const send = vi
			.spyOn(transport, "sendRequest")
			.mockImplementation(async (input) => ({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 1,
				ownership: input.ownership,
				payload: { type: "vm_config_compared", equivalent: false },
			}));
		const process = SidecarProcess.fromClient(transport);
		const before: CreateVmConfig = {
			rootFilesystem: {
				mode: "ephemeral",
				disableDefaultBaseLayer: false,
				lowers: [],
				bootstrapEntries: [],
			},
			loopbackExemptPorts: [],
		};
		const after = { ...before, env: {} };
		try {
			expect(
				await process.compareVmConfig(
					{ connectionId: "conn", sessionId: "session" },
					before,
					after,
				),
			).toBe(false);
			expect(send).toHaveBeenCalledOnce();
			expect(send).toHaveBeenCalledWith({
				ownership: {
					scope: "session",
					connection_id: "conn",
					session_id: "session",
				},
				payload: { type: "compare_vm_config", before, after },
			});
			const rejection = new Error("comparison rejected");
			send.mockRejectedValueOnce(rejection);
			await expect(
				process.compareVmConfig(
					{ connectionId: "conn", sessionId: "session" },
					before,
					after,
				),
			).rejects.toBe(rejection);
		} finally {
			await process.dispose();
			vi.restoreAllMocks();
		}
	});

	test("runs high-level process operations over an injected transport", async () => {
		const transport = new MemorySidecarTransport();
		const process = SidecarProcess.fromClient(transport);

		const layerId = await process.createLayer(
			{ connectionId: "conn", sessionId: "session" },
			{ vmId: "vm" },
		);
		await process.dispose();

		expect(layerId).toBe("layer-from-memory");
		expect(transport.requests).toMatchObject([
			{
				ownership: {
					scope: "vm",
					connection_id: "conn",
					session_id: "session",
					vm_id: "vm",
				},
				payload: { type: "create_layer" },
			},
		]);
		expect(transport.disposed).toBe(true);
	});
});
