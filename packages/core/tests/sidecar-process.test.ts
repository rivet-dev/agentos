import { describe, expect, test, vi } from "vitest";
import type { LiveOwnershipScope } from "../src/ownership.js";
import {
	type LiveEventFrame,
	type LiveResponseFrame,
	type LiveSidecarRequestHandler,
	resolveSidecarRequestFramePayload,
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

	deliverSidecarRequest(ownership: LiveOwnershipScope) {
		return resolveSidecarRequestFramePayload(
			{
				frame_type: "sidecar_request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: -1,
				ownership,
				payload: {
					type: "host_callback",
					invocation_id: "invocation",
					callback_key: "t:who",
					input: {},
					timeout_ms: 1000,
				},
			},
			this.sidecarRequestHandler,
		);
	}

	onEvent(handler: (event: LiveEventFrame) => void): () => void {
		this.eventListeners.add(handler);
		return () => {
			this.eventListeners.delete(handler);
		};
	}

	onClose(): () => void {
		return () => {};
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
	test("answers each sidecar request only from the handler for its owner", async () => {
		const transport = new MemorySidecarTransport();
		const process = SidecarProcess.fromClient(transport);
		const answerAs = (result: string) => async () => ({
			type: "host_callback_result" as const,
			invocation_id: "invocation",
			result,
		});
		const connection = { scope: "connection" as const, connection_id: "conn" };
		const session = {
			...connection,
			scope: "session" as const,
			session_id: "s",
		};
		const vm = (vm_id: string) => ({ ...session, scope: "vm" as const, vm_id });
		process.setSidecarRequestHandler(answerAs("non-vm"));
		process.setVmSidecarRequestHandler("vm-a", answerAs("a"));
		process.setVmSidecarRequestHandler("vm-b", answerAs("b"));

		for (const [owner, result] of [
			[vm("vm-a"), "a"],
			[vm("vm-b"), "b"],
			[connection, "non-vm"],
			[session, "non-vm"],
		] as const) {
			await expect(
				transport.deliverSidecarRequest(owner),
			).resolves.toMatchObject({ result });
		}

		// A removed or unknown VM gets an error, never a sibling or non-VM handler.
		process.setVmSidecarRequestHandler("vm-b", null);
		for (const vmId of ["vm-b", "vm-unknown"]) {
			await expect(transport.deliverSidecarRequest(vm(vmId))).resolves.toEqual({
				type: "host_callback_result",
				invocation_id: "invocation",
				error: `no sidecar request handler registered for host_callback in VM ${vmId}`,
			});
		}
	});

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
				payload: {
					type: "compare_vm_config",
					before,
					after,
					before_mounts: [],
					after_mounts: [],
					before_restart_identity: [],
					after_restart_identity: [],
				},
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
	test("write-files honors advertised connection caps and ordered chunk errors", async () => {
		class BatchTransport extends MemorySidecarTransport {
			constructor(
				readonly maxFrame = 1024 * 1024,
				readonly maxEntries = 2,
			) {
				super();
			}
			override async sendRequest(
				input: Parameters<SidecarProcessTransport["sendRequest"]>[0],
			): Promise<LiveResponseFrame> {
				this.requests.push(input);
				let payload: LiveResponseFrame["payload"];
				switch (input.payload.type) {
					case "authenticate":
						payload = {
							type: "authenticated",
							sidecar_id: "synthetic",
							connection_id: "conn",
							max_frame_bytes: this.maxFrame,
							max_write_files_entries: this.maxEntries,
						};
						break;
					case "open_session":
						payload = {
							type: "session_opened",
							session_id: "session",
							owner_connection_id: "conn",
						};
						break;
					case "guest_filesystem_write_files":
						payload = {
							type: "guest_filesystem_write_files_result",
							response: {
								results: input.payload.entries.map((entry) => ({
									error: entry.path.endsWith("bad")
										? {
												code: "ENOTDIR",
												message: "ENOTDIR: synthetic parent is a file",
											}
										: null,
								})),
							},
						};
						break;
					default:
						throw new Error(
							`unexpected batch test request ${input.payload.type}`,
						);
				}
				return {
					frame_type: "response",
					schema: SIDECAR_PROTOCOL_SCHEMA,
					request_id: this.requests.length,
					ownership: input.ownership,
					payload,
				};
			}
		}
		const transport = new BatchTransport();
		const client = SidecarProcess.fromClient(transport);
		try {
			const session = await client.authenticateAndOpenSession();
			expect(session).toEqual({ connectionId: "conn", sessionId: "session" });
			const content = new Uint8Array([0, 1, 128, 255]);
			const errors = await client.writeFiles(
				session,
				{ vmId: "vm" },
				["a", "bad", "c", "d", "e"].map((name) => ({
					path: `/tmp/${name}`,
					content,
				})),
			);
			expect(errors).toEqual([
				null,
				"ENOTDIR: synthetic parent is a file",
				null,
				null,
				null,
			]);
			const batches = transport.requests.filter(
				(request) => request.payload.type === "guest_filesystem_write_files",
			);
			expect(
				batches.map(
					(request) =>
						request.payload.type === "guest_filesystem_write_files" &&
						request.payload.entries.length,
				),
			).toEqual([2, 2, 1]);
			for (const batch of batches) {
				expect(batch.ownership).toEqual({
					scope: "vm",
					connection_id: "conn",
					session_id: "session",
					vm_id: "vm",
				});
				if (batch.payload.type === "guest_filesystem_write_files")
					expect(batch.payload.entries[0].content).toEqual(content);
			}
		} finally {
			await client.dispose();
		}
		// Each entry fits alone, but two exceed the advertised frame budget.
		const small = new BatchTransport(64 * 1024 + 64, 16);
		const smallClient = SidecarProcess.fromClient(small);
		try {
			const session = await smallClient.authenticateAndOpenSession();
			expect(
				await smallClient.writeFiles(
					session,
					{ vmId: "vm" },
					["a", "b", "c"].map((name) => ({
						path: `/tmp/${name}`,
						content: new Uint8Array(32),
					})),
				),
			).toEqual([null, null, null]);
			expect(
				small.requests.filter(
					(request) => request.payload.type === "guest_filesystem_write_files",
				),
			).toHaveLength(3);
		} finally {
			await smallClient.dispose();
		}
	});

	test("write-files fails closed on missing or malformed handshake capabilities", async () => {
		for (const field of [
			"max_frame_bytes",
			"max_write_files_entries",
		] as const) {
			for (const invalid of [
				undefined,
				0,
				-1,
				1.5,
				Number.NaN,
				Number.POSITIVE_INFINITY,
				2 ** 32,
			]) {
				class MalformedTransport extends MemorySidecarTransport {
					override async sendRequest(
						input: Parameters<SidecarProcessTransport["sendRequest"]>[0],
					): Promise<LiveResponseFrame> {
						this.requests.push(input);
						const payload: unknown = {
							type: "authenticated",
							sidecar_id: "synthetic",
							connection_id: "conn",
							max_frame_bytes: 1024 * 1024,
							max_write_files_entries: 2,
							[field]: invalid,
						};
						return {
							frame_type: "response",
							schema: SIDECAR_PROTOCOL_SCHEMA,
							request_id: 1,
							ownership: input.ownership,
							payload: payload as LiveResponseFrame["payload"],
						};
					}
				}
				const transport = new MalformedTransport();
				const client = SidecarProcess.fromClient(transport);
				try {
					await expect(client.authenticateAndOpenSession()).rejects.toThrow(
						"invalid authenticated write-files limits",
					);
					expect(transport.requests).toHaveLength(1); // no session opened or writes sent.
					expect(transport.failed?.message).toContain(
						"invalid authenticated write-files limits",
					);
				} finally {
					await client.dispose();
				}
			}
		}
	});

	test("write-files reports malformed responses and failed chunks per entry then continues", async () => {
		class BrokenBatchTransport extends MemorySidecarTransport {
			chunks = 0;
			override async sendRequest(
				input: Parameters<SidecarProcessTransport["sendRequest"]>[0],
			): Promise<LiveResponseFrame> {
				this.requests.push(input);
				let payload: LiveResponseFrame["payload"];
				if (input.payload.type === "authenticate")
					payload = {
						type: "authenticated",
						sidecar_id: "synthetic",
						connection_id: "conn",
						max_frame_bytes: 1024 * 1024,
						max_write_files_entries: 2,
					};
				else if (input.payload.type === "open_session")
					payload = {
						type: "session_opened",
						session_id: "session",
						owner_connection_id: "conn",
					};
				else if (input.payload.type === "guest_filesystem_write_files") {
					this.chunks += 1;
					if (this.chunks === 2) throw new Error("synthetic chunk failure");
					payload = {
						type: "guest_filesystem_write_files_result",
						response: {
							results:
								this.chunks === 1
									? []
									: input.payload.entries.map(() => ({ error: null })),
						},
					};
				} else throw new Error(`unexpected request ${input.payload.type}`);
				return {
					frame_type: "response",
					schema: SIDECAR_PROTOCOL_SCHEMA,
					request_id: this.requests.length,
					ownership: input.ownership,
					payload,
				};
			}
		}
		const transport = new BrokenBatchTransport();
		const client = SidecarProcess.fromClient(transport);
		try {
			const session = await client.authenticateAndOpenSession();
			const errors = await client.writeFiles(
				session,
				{ vmId: "vm" },
				[0, 1, 2, 3, 4].map((index) => ({
					path: `/tmp/${index}`,
					content: "x",
				})),
			);
			expect(
				errors
					.slice(0, 2)
					.every((error) =>
						error?.includes("unexpected guest_filesystem_write_files response"),
					),
			).toBe(true);
			expect(errors.slice(2, 4)).toEqual([
				"synthetic chunk failure",
				"synthetic chunk failure",
			]);
			expect(errors[4]).toBeNull();
			expect(transport.chunks).toBe(3);
		} finally {
			await client.dispose();
		}
	});
});
