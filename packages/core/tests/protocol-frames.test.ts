import { describe, expect, it } from "vitest";
import * as protocol from "../src/generated-protocol.js";
import {
	classifySidecarWrittenProtocolFrame,
	decodeBareProtocolFrame,
	decodeProtocolFramePayload,
	encodeBareProtocolFrame,
	encodeProtocolFramePayload,
	fromGeneratedSidecarWrittenProtocolFrame,
	HostProtocolFrameFactory,
	resolveSidecarRequestFramePayload,
	toGeneratedProtocolFrame,
} from "../src/protocol-frames.js";
import { SIDECAR_PROTOCOL_SCHEMA } from "../src/protocol-schema.js";

const textDecoder = new TextDecoder();

const ownership = {
	scope: "connection" as const,
	connection_id: "conn",
};

const generatedAuthOwnership = {
	scope: "connection" as const,
	connection_id: "conn-1",
};

const GENERATED_AUTH_FRAME_HEX =
	"000f6167656e746f732d736964656361720a0007000000000000000006636f6e6e2d31000e67656e6572617465642d7465737405746f6b656e0a0001000000";

const hostCallbackRequest = {
	frame_type: "sidecar_request" as const,
	schema: SIDECAR_PROTOCOL_SCHEMA,
	request_id: 7,
	ownership,
	payload: {
		type: "host_callback" as const,
		invocation_id: "invocation",
		callback_key: "tool",
		input: {},
		timeout_ms: 1000,
	},
};

describe("protocol frame conversion", () => {
	it("roundtrips both package sources, omitted limits, and exact acquisition sizes", () => {
		for (const source of [
			{
				type: "url" as const,
				url: "https://packages.gameinc.io/tool.aospkg",
				expected_digest: "sha256:abc",
			},
			{ type: "path" as const, path: "/trusted/tool.aospkg" },
		]) {
			const session = {
				scope: "session" as const,
				connection_id: "conn",
				session_id: "session",
			};
			const decoded = protocol.decodeProtocolFrame(
				encodeBareProtocolFrame({
					frame_type: "request",
					schema: SIDECAR_PROTOCOL_SCHEMA,
					request_id: 12,
					ownership: session,
					payload: {
						type: "acquire_package",
						source,
						timeout_ms: 250n,
						max_package_bytes: 9007199254740993n,
					},
				}),
			);
			if (
				decoded.tag !== "RequestFrame" ||
				decoded.val.payload.tag !== "AcquirePackageRequest"
			) {
				throw new Error("wrong acquisition request frame");
			}
			expect(decoded.val.payload.val).toEqual({
				source:
					source.type === "url"
						? {
								tag: "PackageUrlSource",
								val: {
									url: source.url,
									expectedDigest: source.expected_digest,
								},
							}
						: {
								tag: "PackagePathSource",
								val: { path: source.path, expectedDigest: null },
							},
				advisory: false,
				timeoutMs: 250n,
				maxPackageBytes: 9007199254740993n,
				downloadTimeoutMs: null,
				connectTimeoutMs: null,
				maxRedirects: null,
				allowInsecureLocalHttp: false,
			});
			const metadata = {
				packageId: "sha256:abc",
				digest: "sha256:abc",
				size: 9007199254740993n,
				packageName: "tool",
				version: "1",
				commands: ["tool"],
			};
			expect(
				decodeBareProtocolFrame(
					protocol.encodeProtocolFrame({
						tag: "ResponseFrame",
						val: {
							schema: SIDECAR_PROTOCOL_SCHEMA,
							requestId: 12n,
							ownership: decoded.val.ownership,
							payload: { tag: "PackageAcquiredResponse", val: metadata },
						},
					}),
				),
			).toEqual({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 12,
				ownership: session,
				payload: {
					type: "package_acquired",
					package_id: metadata.packageId,
					digest: metadata.digest,
					size: metadata.size,
					package_name: metadata.packageName,
					version: metadata.version,
					commands: metadata.commands,
				},
			});
		}
	});

	it("roundtrips VM package installation and returns no host path", () => {
		const vm = {
			scope: "vm" as const,
			connection_id: "conn",
			session_id: "session",
			vm_id: "vm",
		};
		const decoded = protocol.decodeProtocolFrame(
			encodeBareProtocolFrame({
				frame_type: "request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 14,
				ownership: vm,
				payload: {
					type: "install_package",
					acquisition: {
						source: {
							type: "url",
							url: "https://packages.gameinc.io/tool.aospkg",
						},
						max_package_bytes: 9007199254740993n,
					},
				},
			}),
		);
		if (
			decoded.tag !== "RequestFrame" ||
			decoded.val.payload.tag !== "InstallPackageRequest"
		) {
			throw new Error("wrong installation request frame");
		}
		expect(decoded.val.payload.val.acquisition.advisory).toBe(false);
		expect(decoded.val.payload.val.acquisition.maxPackageBytes).toBe(
			9007199254740993n,
		);
		const response = decodeBareProtocolFrame(
			protocol.encodeProtocolFrame({
				tag: "ResponseFrame",
				val: {
					schema: SIDECAR_PROTOCOL_SCHEMA,
					requestId: 14n,
					ownership: decoded.val.ownership,
					payload: {
						tag: "PackageInstalledResponse",
						val: {
							package: {
								packageId: "sha256:abc",
								digest: "sha256:abc",
								size: 9007199254740993n,
								packageName: "tool",
								version: "1",
								commands: ["tool"],
							},
							projectedCommands: [
								{ name: "tool", guestPath: "/opt/agentos/bin/tool" },
							],
						},
					},
				},
			}),
		);
		expect(response.payload).toEqual({
			type: "package_installed",
			package: {
				package_id: "sha256:abc",
				digest: "sha256:abc",
				size: 9007199254740993n,
				package_name: "tool",
				version: "1",
				commands: ["tool"],
			},
			projected_commands: [
				{ name: "tool", guest_path: "/opt/agentos/bin/tool" },
			],
		});
	});

	it("roundtrips session-scoped config comparison through BARE", () => {
		const session = {
			scope: "session" as const,
			connection_id: "conn",
			session_id: "session",
		};
		const before = {
			defaultsProfile: "agent_os" as const,
			rootFilesystem: {
				mode: "ephemeral" as const,
				disableDefaultBaseLayer: false,
				lowers: [],
				bootstrapEntries: [],
			},
			loopbackExemptPorts: [],
		};
		const after = {
			...before,
			env: {},
			jsRuntime: {
				platform: "node" as const,
				moduleResolution: "node" as const,
				allowedBuiltins: [],
			},
		};
		const decoded = protocol.decodeProtocolFrame(
			encodeBareProtocolFrame({
				frame_type: "request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 10,
				ownership: session,
				payload: { type: "compare_vm_config", before, after },
			}),
		);
		expect(decoded.tag).toBe("RequestFrame");
		if (
			decoded.tag !== "RequestFrame" ||
			decoded.val.payload.tag !== "CompareVmConfigRequest"
		)
			throw new Error("wrong request frame");
		expect(decoded.val.ownership).toEqual({
			tag: "SessionOwnership",
			val: { connectionId: "conn", sessionId: "session" },
		});
		expect(JSON.parse(decoded.val.payload.val.before)).toEqual(before);
		expect(JSON.parse(decoded.val.payload.val.after)).toEqual(after);
		for (const equivalent of [true, false]) {
			expect(
				decodeBareProtocolFrame(
					protocol.encodeProtocolFrame({
						tag: "ResponseFrame",
						val: {
							schema: SIDECAR_PROTOCOL_SCHEMA,
							requestId: 10n,
							ownership: decoded.val.ownership,
							payload: { tag: "VmConfigComparedResponse", val: { equivalent } },
						},
					}),
				),
			).toEqual({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 10,
				ownership: session,
				payload: { type: "vm_config_compared", equivalent },
			});
		}
	});

	it("creates host-written request, response, and control frames", () => {
		const factory = new HostProtocolFrameFactory();

		const first = factory.createRequestFrame({
			ownership,
			payload: {
				type: "authenticate",
				client_name: "agentos",
				auth_token: "token",
				protocol_version: 10,
				bridge_version: 1,
			},
		});
		const second = factory.createRequestFrame({
			ownership,
			payload: {
				type: "create_layer",
			},
		});

		expect(first).toMatchObject({
			frame_type: "request",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 1,
			ownership,
		});
		expect(second.request_id).toBe(2);
		expect(
			factory.createSidecarResponseFrame({
				request: hostCallbackRequest,
				payload: {
					type: "host_callback_result",
					invocation_id: "invocation",
					result: { ok: true },
				},
			}),
		).toEqual({
			frame_type: "sidecar_response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 7,
			ownership,
			payload: {
				type: "host_callback_result",
				invocation_id: "invocation",
				result: { ok: true },
			},
		});
		expect(
			factory.createControlFrame({ type: "shutdown", reason: "test complete" }),
		).toEqual({
			frame_type: "control",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			payload: { type: "shutdown", reason: "test complete" },
		});
	});

	it("resolves sidecar request frame handlers", async () => {
		await expect(
			resolveSidecarRequestFramePayload(hostCallbackRequest, async () => ({
				type: "host_callback_result",
				invocation_id: "invocation",
				result: { ok: true },
			})),
		).resolves.toEqual({
			type: "host_callback_result",
			invocation_id: "invocation",
			result: { ok: true },
		});
	});

	it("returns error payloads for missing or mismatched sidecar handlers", async () => {
		await expect(
			resolveSidecarRequestFramePayload(hostCallbackRequest, null),
		).resolves.toMatchObject({
			type: "host_callback_result",
			invocation_id: "invocation",
			error: "no sidecar request handler registered for host_callback",
		});

		await expect(
			resolveSidecarRequestFramePayload(hostCallbackRequest, async () => ({
				type: "js_bridge_result",
				call_id: "call",
				result: {},
			})),
		).resolves.toMatchObject({
			type: "host_callback_result",
			invocation_id: "invocation",
			error: "sidecar handler returned js_bridge_result for host_callback",
		});
	});

	it("maps host-written request frames to generated protocol frames", () => {
		expect(
			toGeneratedProtocolFrame({
				frame_type: "request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 7,
				ownership,
				payload: {
					type: "authenticate",
					client_name: "agentos",
					auth_token: "token",
					protocol_version: 10,
					bridge_version: 1,
				},
			}),
		).toMatchObject({
			tag: "RequestFrame",
			val: {
				requestId: 7n,
				payload: {
					tag: "AuthenticateRequest",
				},
			},
		});
	});

	it("encodes host-written frames as BARE protocol bytes", () => {
		const encoded = encodeBareProtocolFrame({
			frame_type: "sidecar_response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 8,
			ownership,
			payload: {
				type: "host_callback_result",
				invocation_id: "invocation",
				result: { ok: true },
			},
		});

		expect(protocol.decodeProtocolFrame(new Uint8Array(encoded)).tag).toBe(
			"SidecarResponseFrame",
		);

		const control = encodeBareProtocolFrame({
			frame_type: "control",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			payload: { type: "shutdown", reason: "test complete" },
		});
		expect(protocol.decodeProtocolFrame(new Uint8Array(control))).toMatchObject(
			{
				tag: "ControlFrame",
				val: {
					payload: {
						tag: "ShutdownControl",
						val: { reason: "test complete" },
					},
				},
			},
		);
	});

	it("matches native generated auth frame BARE bytes", () => {
		const encoded = encodeBareProtocolFrame({
			frame_type: "request",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 7,
			ownership: generatedAuthOwnership,
			payload: {
				type: "authenticate",
				client_name: "generated-test",
				auth_token: "token",
				protocol_version: SIDECAR_PROTOCOL_SCHEMA.version,
				bridge_version: 1,
			},
		});

		expect(Buffer.from(encoded).toString("hex")).toBe(GENERATED_AUTH_FRAME_HEX);
		expect(protocol.decodeProtocolFrame(new Uint8Array(encoded))).toMatchObject(
			{
				tag: "RequestFrame",
				val: {
					requestId: 7n,
					payload: { tag: "AuthenticateRequest" },
				},
			},
		);
	});

	it("decodes sidecar-written response frames from generated protocol frames", () => {
		expect(
			fromGeneratedSidecarWrittenProtocolFrame({
				tag: "ResponseFrame",
				val: {
					schema: SIDECAR_PROTOCOL_SCHEMA,
					requestId: 9n,
					ownership: {
						tag: "ConnectionOwnership",
						val: { connectionId: "conn" },
					},
					payload: {
						tag: "VmCreatedResponse",
						val: { vmId: "vm" },
					},
				},
			}),
		).toEqual({
			frame_type: "response",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			request_id: 9,
			ownership,
			payload: { type: "vm_created", vm_id: "vm" },
		});
	});

	it("decodes sidecar-written event frames from BARE bytes", () => {
		const bytes = protocol.encodeProtocolFrame({
			tag: "EventFrame",
			val: {
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership: {
					tag: "ConnectionOwnership",
					val: { connectionId: "conn" },
				},
				payload: {
					tag: "StructuredEvent",
					val: {
						name: "ready",
						detail: new Map([["ok", "true"]]),
					},
				},
			},
		});

		expect(decodeBareProtocolFrame(bytes)).toEqual({
			frame_type: "event",
			schema: SIDECAR_PROTOCOL_SCHEMA,
			ownership,
			payload: {
				type: "structured",
				name: "ready",
				detail: { ok: "true" },
			},
		});
	});

	it("encodes and decodes JSON compatibility protocol frames", () => {
		const encoded = encodeProtocolFramePayload(
			{
				frame_type: "event",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership,
				payload: {
					type: "process_output",
					process_id: "proc",
					channel: "stdout",
					chunk: new Uint8Array([1, 2, 3]),
				},
			},
			"json",
		);

		expect(JSON.parse(textDecoder.decode(encoded)).payload.chunk).toEqual([
			1, 2, 3,
		]);

		const decoded = decodeProtocolFramePayload(encoded, "json");
		if (
			decoded.frame_type !== "event" ||
			decoded.payload.type !== "process_output"
		) {
			throw new Error("expected process_output event");
		}
		expect(decoded.payload.chunk).toBeInstanceOf(Uint8Array);
		expect(Array.from(decoded.payload.chunk)).toEqual([1, 2, 3]);
	});

	it("classifies sidecar-written protocol frames for RPC dispatch", () => {
		expect(
			classifySidecarWrittenProtocolFrame({
				frame_type: "response",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 10,
				ownership,
				payload: { type: "vm_created", vm_id: "vm" },
			}),
		).toMatchObject({
			kind: "response",
			requestId: 10,
			frame: { frame_type: "response" },
		});

		expect(
			classifySidecarWrittenProtocolFrame({
				frame_type: "event",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				ownership,
				payload: {
					type: "structured",
					name: "ready",
					detail: {},
				},
			}),
		).toMatchObject({
			kind: "event",
			frame: { frame_type: "event" },
		});

		expect(
			classifySidecarWrittenProtocolFrame({
				frame_type: "sidecar_request",
				schema: SIDECAR_PROTOCOL_SCHEMA,
				request_id: 11,
				ownership,
				payload: {
					type: "host_callback",
					invocation_id: "invocation",
					callback_key: "tool",
					input: { ok: true },
					timeout_ms: 1_000,
				},
			}),
		).toMatchObject({
			kind: "sidecarRequest",
			frame: { frame_type: "sidecar_request" },
		});
	});

	it("rejects host-written generated frames on the sidecar-written decode path", () => {
		expect(() =>
			fromGeneratedSidecarWrittenProtocolFrame({
				tag: "RequestFrame",
				val: {
					schema: SIDECAR_PROTOCOL_SCHEMA,
					requestId: 1n,
					ownership: {
						tag: "ConnectionOwnership",
						val: { connectionId: "conn" },
					},
					payload: {
						tag: "CreateLayerRequest",
						val: null,
					},
				},
			}),
		).toThrow("unsupported BARE protocol frame tag: RequestFrame");
	});
});
