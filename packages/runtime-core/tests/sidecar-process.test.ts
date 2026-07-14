import { describe, expect, test } from "vitest";
import type { SidecarProcessTransport } from "../src/sidecar-client.js";
import { SidecarProcess } from "../src/sidecar-process.js";
import type {
	LiveEventFrame,
	LiveResponseFrame,
	LiveSidecarRequestHandler,
} from "../src/protocol-frames.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";
import type { LiveOwnershipScope } from "../src/ownership.js";
import type { LiveRequestPayload } from "../src/request-payloads.js";

class MemorySidecarTransport implements SidecarProcessTransport {
	readonly requests: Array<{
		ownership: LiveOwnershipScope;
		payload: LiveRequestPayload;
	}> = [];
	disposed = false;
	failed: Error | null = null;
	private readonly sidecarRequestHandlers = new Map<
		string,
		LiveSidecarRequestHandler
	>();
	private readonly eventListeners = new Set<(event: LiveEventFrame) => void>();

	registerSidecarRequestHandler(
		ownership: LiveOwnershipScope,
		handler: LiveSidecarRequestHandler,
	): () => void {
		const key = JSON.stringify(ownership);
		this.sidecarRequestHandlers.set(key, handler);
		return () => this.sidecarRequestHandlers.delete(key);
	}

	onEvent(
		handler: (event: LiveEventFrame) => void,
		_ownership?: LiveOwnershipScope,
	): () => void {
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
		if (input.payload.type === "initialize_vm") {
			return {
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: this.requests.length,
				ownership: input.ownership,
				payload: {
					type: "vm_initialized",
					vm_id: "vm-initialized",
					guest_cwd: "/workspace",
					guest_env: { HOME: "/home/agentos" },
					applied_mounts: 0,
					projected_commands: [],
					agents: [],
					host_callbacks: [],
					process_route_retention: 1024,
				},
			};
		}
		if (input.payload.type === "guest_filesystem_call") {
			return {
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: this.requests.length,
				ownership: input.ownership,
				payload: {
					type: "guest_filesystem_result",
					operation: input.payload.operation,
					path: input.payload.path,
				},
			};
		}
		if (input.payload.type === "close_session") {
			return {
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: this.requests.length,
				ownership: input.ownership,
				payload: {
					type: "session_closed",
					session_id: input.payload.session_id,
				},
			};
		}
		if (input.payload.type === "execute") {
			return {
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: this.requests.length,
				ownership: input.ownership,
				payload: {
					type: "process_started",
					process_id: `process-${this.requests.length}`,
					pid: 42,
				},
			};
		}
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
	test("forwards one initialization request and preserves omissions", async () => {
		const transport = new MemorySidecarTransport();
		const process = SidecarProcess.fromClient(transport);

		const initialized = await process.initializeVm(
			{ connectionId: "conn", sessionId: "session" },
			{ runtime: "java_script", config: {} },
		);

		expect(initialized).toMatchObject({
			vmId: "vm-initialized",
			guestCwd: "/workspace",
			guestEnv: { HOME: "/home/agentos" },
			processRouteRetention: 1024,
		});
		expect(transport.requests).toEqual([
			{
				ownership: {
					scope: "session",
					connection_id: "conn",
					session_id: "session",
				},
				payload: {
					type: "initialize_vm",
					runtime: "java_script",
					config: {},
				},
			},
		]);
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

	test("closes a session through connection ownership", async () => {
		const transport = new MemorySidecarTransport();
		const process = SidecarProcess.fromClient(transport);

		await process.closeSession({
			connectionId: "conn",
			sessionId: "session",
		});

		expect(transport.requests).toEqual([
			{
				ownership: {
					scope: "connection",
					connection_id: "conn",
				},
				payload: {
					type: "close_session",
					session_id: "session",
				},
			},
		]);
	});

	test("rejects missing filesystem response payloads instead of inventing results", async () => {
		const process = SidecarProcess.fromClient(new MemorySidecarTransport());
		const session = { connectionId: "conn", sessionId: "session" };
		const vm = { vmId: "vm" };

		await expect(process.exists(session, vm, "/missing")).rejects.toThrow(
			"sidecar returned no exists result for /missing",
		);
		await expect(process.readdir(session, vm, "/empty")).rejects.toThrow(
			"sidecar returned no directory entries for /empty",
		);
		await expect(
			process.readdirRecursive(session, vm, "/empty"),
		).rejects.toThrow(
			"sidecar returned no recursive directory entries for /empty",
		);
	});

	test("preserves false, true, and omission for keepStdinOpen", async () => {
		const transport = new MemorySidecarTransport();
		const process = SidecarProcess.fromClient(transport);
		const session = { connectionId: "conn", sessionId: "session" };
		const vm = { vmId: "vm" };

		await process.execute(session, vm, {
			command: "false-case",
			keepStdinOpen: false,
		});
		await process.execute(session, vm, {
			command: "true-case",
			keepStdinOpen: true,
		});
		await process.execute(session, vm, { command: "omitted-case" });

		const executePayloads = transport.requests
			.map((request) => request.payload)
			.filter((payload) => payload.type === "execute");
		expect(executePayloads).toHaveLength(3);
		expect(executePayloads[0]).toEqual(
			expect.objectContaining({
				type: "execute",
				command: "false-case",
				keep_stdin_open: false,
			}),
		);
		expect(executePayloads[1]).toEqual(
			expect.objectContaining({
				type: "execute",
				command: "true-case",
				keep_stdin_open: true,
			}),
		);
		expect(executePayloads[2]).not.toHaveProperty("keep_stdin_open");
	});
});
