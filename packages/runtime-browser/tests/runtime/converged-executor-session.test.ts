import { describe, expect, it } from "vitest";
import type { LiveOwnershipScope } from "@rivet-dev/agentos-runtime-core/ownership";
import {
	decodeProtocolFramePayload,
	encodeProtocolFramePayload,
	type LiveProtocolFrame,
	type LiveResponsePayload,
} from "@rivet-dev/agentos-runtime-core/protocol-frames";
import type { LiveRequestPayload } from "@rivet-dev/agentos-runtime-core/request-payloads";
import { SIDECAR_PROTOCOL_SCHEMA } from "@rivet-dev/agentos-runtime-core/protocol-schema";
import { ConvergedExecutorSession } from "../../src/converged-executor-session.js";
import { SYNC_BRIDGE_KIND_TEXT } from "../../src/sync-bridge.js";

// A fake wasm sidecar that drives the full lifecycle handshake + guest fs over
// JSON-encoded frames (no binary payloads), recording the ownership each request
// arrives with.
function fakeSidecar(): {
	pushFrame: (frame: Uint8Array) => Uint8Array;
	ownerships: LiveOwnershipScope[];
	requests: LiveRequestPayload[];
} {
	const ownerships: LiveOwnershipScope[] = [];
	const requests: LiveRequestPayload[] = [];
	const pushFrame = (frameBytes: Uint8Array): Uint8Array => {
		const frame = decodeProtocolFramePayload(
			frameBytes,
			"json",
		) as unknown as LiveProtocolFrame;
		if (frame.frame_type !== "request") {
			throw new Error(`expected request, got ${frame.frame_type}`);
		}
		ownerships.push(frame.ownership);
		requests.push(frame.payload);
		return encodeProtocolFramePayload(
			{
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: frame.request_id,
				ownership: frame.ownership,
				payload: service(frame.payload),
			},
			"json",
		);
	};
	return { pushFrame, ownerships, requests };
}

function service(request: LiveRequestPayload): LiveResponsePayload {
	switch (request.type) {
		case "authenticate":
			return {
				type: "authenticated",
				sidecar_id: "sidecar",
				connection_id: "conn-1",
				max_frame_bytes: 1024,
			};
		case "open_session":
			return {
				type: "session_opened",
				session_id: "session-1",
				owner_connection_id: "conn-1",
			};
		case "create_vm":
			return { type: "vm_created", vm_id: "vm-1" };
		case "initialize_vm":
			return {
				type: "vm_initialized",
				vm_id: "vm-1",
				guest_cwd: "/",
				guest_env: {},
				process_route_retention: 1,
				applied_mounts: 1,
				projected_commands: [],
				agents: [],
				host_callbacks: [],
			};
		case "execute":
			return { type: "process_started", process_id: request.process_id };
		case "guest_filesystem_call":
			return {
				type: "guest_filesystem_result",
				operation: "read_file",
				path: request.path,
				content: "converged",
				encoding: "utf8",
			};
		default:
			return { type: "rejected", code: "unexpected", message: request.type };
	}
}

describe("converged executor session", () => {
	it("runs the authenticate/open_session/create_vm handshake with correct ownership", () => {
		const sidecar = fakeSidecar();
		const session = new ConvergedExecutorSession({
			pushFrame: sidecar.pushFrame,
			codec: "json",
		});
		const vm = session.bootstrap({
			runtime: "java_script",
			config: {} as never,
		});
		expect(vm).toEqual({
			connectionId: "conn-1",
			sessionId: "session-1",
			vmId: "vm-1",
		});
		expect(sidecar.ownerships.map((o) => o.scope)).toEqual([
			"connection",
			"connection",
			"session",
		]);
	});

	it("hands out a VM-scoped execution handler after bootstrap", () => {
		const sidecar = fakeSidecar();
		const session = new ConvergedExecutorSession({
			pushFrame: sidecar.pushFrame,
			codec: "json",
		});
		session.bootstrap({ runtime: "java_script", config: {} as never });
		const handler = session.handlerForExecution("exec-1");
		const response = handler.handle("fs.readFile", ["/tmp/x"]);
		expect(response).toEqual({
			kind: SYNC_BRIDGE_KIND_TEXT,
			value: "converged",
		});
		// The guest syscall arrives with VM ownership.
		const last = sidecar.ownerships.at(-1);
		expect(last).toEqual({
			scope: "vm",
			connection_id: "conn-1",
			session_id: "session-1",
			vm_id: "vm-1",
		});
	});

	it("forwards opaque package bytes through atomic VM initialization", () => {
		const sidecar = fakeSidecar();
		const session = new ConvergedExecutorSession({
			pushFrame: sidecar.pushFrame,
			codec: "json",
		});
		session.bootstrap({
			runtime: "java_script",
			config: {} as never,
			packages: [{ content: new Uint8Array([1, 2, 3]) }],
			packagesMountAt: "/srv/agentos",
		});

		const request = sidecar.requests.at(-1);
		expect(request).toMatchObject({
			type: "initialize_vm",
			runtime: "java_script",
			config: {},
		});
		if (request?.type !== "initialize_vm") {
			throw new Error("expected initialize_vm request");
		}
		expect(Array.from(request.packages?.[0]?.content ?? [])).toEqual([1, 2, 3]);
		expect(request.packages_mount_at).toBe("/srv/agentos");
	});

	it("registers a guest execution via an execute wire request", () => {
		const sidecar = fakeSidecar();
		const session = new ConvergedExecutorSession({
			pushFrame: sidecar.pushFrame,
			codec: "json",
		});
		session.bootstrap({ runtime: "java_script", config: {} as never });
		const registered = session.registerExecution({
			processId: "exec-7",
			entrypoint: "/main.js",
			args: ["main.js"],
		});
		expect(registered).toEqual({ processId: "exec-7" });
		// The execute request arrives with VM ownership.
		expect(sidecar.ownerships.at(-1)).toMatchObject({
			scope: "vm",
			vm_id: "vm-1",
		});
	});

	it("throws if a handler is requested before bootstrap", () => {
		const session = new ConvergedExecutorSession({
			pushFrame: fakeSidecar().pushFrame,
			codec: "json",
		});
		expect(() => session.handlerForExecution("exec-1")).toThrow(
			/has not bootstrapped/,
		);
	});
});
