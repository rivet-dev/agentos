import { describe, expect, it } from "vitest";
import * as protocol from "../src/generated-protocol.js";
import { toGeneratedRequestPayload } from "../src/request-payloads.js";

describe("request payload conversion", () => {
	it("maps authentication and session requests", () => {
		expect(
			toGeneratedRequestPayload({
				type: "authenticate",
				client_name: "agentos",
				auth_token: "token",
				protocol_version: 7,
				bridge_version: 1,
			}),
		).toEqual({
			tag: "AuthenticateRequest",
			val: {
				clientName: "agentos",
				authToken: "token",
				protocolVersion: 7,
				bridgeVersion: 1,
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "open_session",
				placement: { kind: "shared", pool: "default" },
			}),
		).toEqual({
			tag: "OpenSessionRequest",
			val: {
				placement: {
					tag: "SidecarPlacementShared",
					val: { pool: "default" },
				},
			},
		});
	});

	it("preserves an omitted overlay mode for the sidecar default", () => {
		expect(
			toGeneratedRequestPayload({
				type: "create_overlay",
				lower_layer_ids: [],
			}),
		).toEqual({
			tag: "CreateOverlayRequest",
			val: {
				mode: null,
				upperLayerId: null,
				lowerLayerIds: [],
			},
		});
	});

	it("maps VM creation and configuration requests", () => {
		const createVmPayload = toGeneratedRequestPayload({
			type: "create_vm",
			runtime: "java_script",
			config: {
				env: {},
				rootFilesystem: {
					mode: "read-only",
					disableDefaultBaseLayer: true,
					lowers: [],
					bootstrapEntries: [{ path: "/tmp", kind: "directory" }],
				},
				permissions: { fs: "allow" },
				loopbackExemptPorts: [],
			},
		});
		expect(createVmPayload).toMatchObject({
			tag: "CreateVmRequest",
			val: {
				runtime: protocol.GuestRuntimeKind.JavaScript,
			},
		});
		expect(JSON.parse(createVmPayload.val.config)).toMatchObject({
			rootFilesystem: {
				mode: "read-only",
				disableDefaultBaseLayer: true,
				bootstrapEntries: [{ path: "/tmp", kind: "directory" }],
			},
			permissions: { fs: "allow" },
		});

		expect(
			toGeneratedRequestPayload({
				type: "configure_vm",
				mounts: [
					{
						guest_path: "/workspace",
						read_only: true,
						plugin: { id: "host", config: { source: "/src" } },
					},
				],
				command_permissions: { node: "read-only" },
				loopback_exempt_ports: [8080],
			}),
		).toMatchObject({
			tag: "ConfigureVmRequest",
			val: {
				commandPermissions: new Map([
					["node", protocol.WasmPermissionTier.ReadOnly],
				]),
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "configure_vm",
			}),
		).toMatchObject({
			tag: "ConfigureVmRequest",
			val: {
				mounts: null,
				commandPermissions: null,
				loopbackExemptPorts: null,
				packages: null,
				packagesMountAt: null,
			},
		});
	});

	it("preserves omitted initialization fields for sidecar defaults", () => {
		const payload = toGeneratedRequestPayload({
			type: "initialize_vm",
			runtime: "java_script",
			config: {},
			host_callbacks: [
				{
					name: "tools",
					description: "demo tools",
					callbacks: {
						read: {
							description: "read a file",
							input_schema: { type: "object" },
						},
					},
				},
			],
		});

		expect(payload).toEqual({
			tag: "InitializeVmRequest",
			val: {
				runtime: protocol.GuestRuntimeKind.JavaScript,
				config: "{}",
				mounts: null,
				packages: null,
				packagesMountAt: null,
				hostCallbacks: [
					{
						name: "tools",
						description: "demo tools",
						callbacks: new Map([
							[
								"read",
								{
									description: "read a file",
									inputSchema: '{"type":"object"}',
									timeoutMs: null,
									examples: [],
								},
							],
						]),
					},
				],
			},
		});

		const jsOverridePayload = toGeneratedRequestPayload({
			type: "initialize_vm",
			runtime: "java_script",
			config: {
				jsRuntime: {
					allowedBuiltins: ["path"],
					highResolutionTime: true,
				},
			},
		});
		expect(jsOverridePayload.val.hostCallbacks).toBeNull();
		expect(JSON.parse(jsOverridePayload.val.config)).toEqual({
			jsRuntime: {
				allowedBuiltins: ["path"],
				highResolutionTime: true,
			},
		});
	});

	it("maps resource snapshot requests", () => {
		expect(
			toGeneratedRequestPayload({
				type: "get_resource_snapshot",
			}),
		).toEqual({ tag: "GetResourceSnapshotRequest", val: null });
	});

	it("preserves omitted cron defaults for the sidecar", () => {
		expect(
			toGeneratedRequestPayload({
				type: "schedule_cron",
				schedule: "* * * * *",
				action: { type: "exec", command: "true" },
			}),
		).toEqual({
			tag: "ScheduleCronRequest",
			val: {
				id: null,
				schedule: "* * * * *",
				action: '{"type":"exec","command":"true"}',
				overlap: null,
			},
		});
		expect(
			toGeneratedRequestPayload({
				type: "schedule_cron",
				id: "job",
				schedule: "* * * * *",
				action: { type: "exec", command: "true" },
				overlap: "queue",
			}),
		).toMatchObject({
			tag: "ScheduleCronRequest",
			val: { id: "job", overlap: protocol.CronOverlap.Queue },
		});
		expect(toGeneratedRequestPayload({ type: "export_cron_state" })).toEqual({
			tag: "ExportCronStateRequest",
			val: null,
		});
		const state = '{"version":1}';
		expect(
			toGeneratedRequestPayload({ type: "import_cron_state", state }),
		).toEqual({
			tag: "ImportCronStateRequest",
			val: { state },
		});
	});

	it("maps host callback registration JSON fields", () => {
		expect(
			toGeneratedRequestPayload({
				type: "register_host_callbacks",
				name: "tools",
				description: "demo tools",
				callbacks: {
					read: {
						description: "read a file",
						input_schema: { type: "object" },
						timeout_ms: 2500,
						examples: [{ description: "basic", input: { path: "/tmp" } }],
					},
				},
			}),
		).toEqual({
			tag: "RegisterHostCallbacksRequest",
			val: {
				name: "tools",
				description: "demo tools",
				callbacks: new Map([
					[
						"read",
						{
							description: "read a file",
							inputSchema: '{"type":"object"}',
							timeoutMs: 2500n,
							examples: [
								{
									description: "basic",
									input: '{"path":"/tmp"}',
								},
							],
						},
					],
				]),
			},
		});
	});

	it("maps filesystem and process requests", () => {
		expect(
			toGeneratedRequestPayload({
				type: "guest_filesystem_call",
				operation: "pread",
				path: "/tmp/file",
				len: 10,
				offset: 2,
				encoding: "base64",
			}),
		).toEqual({
			tag: "GuestFilesystemCallRequest",
			val: {
				operation: protocol.GuestFilesystemOperation.Pread,
				path: "/tmp/file",
				destinationPath: null,
				target: null,
				content: null,
				encoding: protocol.RootFilesystemEntryEncoding.BasE64,
				recursive: null,
				mode: null,
				uid: null,
				gid: null,
				atimeMs: null,
				mtimeMs: null,
				len: 10n,
				offset: 2n,
				maxDepth: null,
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "execute",
				process_id: "proc",
				command: "node",
				args: ["-e", "0"],
				env: { A: "1" },
				runtime: "java_script",
				wasm_permission_tier: "isolated",
				timeout_ms: 25,
			}),
		).toEqual({
			tag: "ExecuteRequest",
			val: {
				processId: "proc",
				command: "node",
				shellCommand: null,
				runtime: protocol.GuestRuntimeKind.JavaScript,
				entrypoint: null,
				args: ["-e", "0"],
				env: new Map([["A", "1"]]),
				cwd: null,
				wasmPermissionTier: protocol.WasmPermissionTier.Isolated,
				pty: null,
				keepStdinOpen: null,
				timeoutMs: 25n,
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "execute",
				command: "node",
				args: [],
			}),
		).toMatchObject({
			tag: "ExecuteRequest",
			val: {
				processId: null,
				shellCommand: null,
				env: null,
				cwd: null,
				pty: null,
				keepStdinOpen: null,
				timeoutMs: null,
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "execute",
				process_id: "proc-command-line",
				shell_command: "echo a && echo b",
				args: [],
			}),
		).toMatchObject({
			tag: "ExecuteRequest",
			val: {
				command: null,
				shellCommand: "echo a && echo b",
			},
		});
	});

	it("maps guest kernel call requests", () => {
		const payload = new TextEncoder().encode(
			JSON.stringify({ host: "127.0.0.1", port: 39221 }),
		).buffer;
		expect(
			toGeneratedRequestPayload({
				type: "guest_kernel_call",
				execution_id: "exec-1",
				operation: "net.connect",
				payload,
			}),
		).toEqual({
			tag: "GuestKernelCallRequest",
			val: {
				executionId: "exec-1",
				operation: "net.connect",
				payload,
			},
		});
	});

	it("maps stdin and ext requests", () => {
		expect(
			toGeneratedRequestPayload({
				type: "write_stdin",
				process_id: "proc",
				chunk: new Uint8Array([1, 2, 3]),
			}),
		).toMatchObject({
			tag: "WriteStdinRequest",
			val: {
				processId: "proc",
			},
		});

		expect(
			toGeneratedRequestPayload({
				type: "ext",
				envelope: {
					namespace: "dev.test",
					payload: new Uint8Array([4, 5]),
				},
			}),
		).toMatchObject({
			tag: "ExtEnvelope",
			val: {
				namespace: "dev.test",
			},
		});
	});
});
