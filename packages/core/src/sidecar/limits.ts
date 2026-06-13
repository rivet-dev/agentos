import type { AgentOsLimits } from "../agent-os.js";

/**
 * Convert `AgentOsLimits` into the flat `CreateVmRequest.metadata` string entries the native
 * sidecar parses. Kernel resource fields use the existing `resource.*` keys; every other group
 * uses `limits.<group>.<field>` snake_case keys to match `crates/sidecar/src/limits.rs`.
 *
 * This is a pure function (no VM, no I/O) so it is unit-testable in isolation. Unknown, negative,
 * or non-integer values throw `AgentOsLimitsError` here, at `AgentOs.create()` time, rather than
 * failing later at first enforcement.
 */
export class AgentOsLimitsError extends Error {
	constructor(message: string) {
		super(message);
		this.name = "AgentOsLimitsError";
	}
}

/** Kernel resource fields keep their historical `resource.*` metadata keys. */
const RESOURCE_KEYS: Record<
	keyof NonNullable<AgentOsLimits["resources"]>,
	string
> = {
	cpuCount: "resource.cpu_count",
	maxProcesses: "resource.max_processes",
	maxOpenFds: "resource.max_open_fds",
	maxPipes: "resource.max_pipes",
	maxPtys: "resource.max_ptys",
	maxSockets: "resource.max_sockets",
	maxConnections: "resource.max_connections",
	maxSocketBufferedBytes: "resource.max_socket_buffered_bytes",
	maxSocketDatagramQueueLen: "resource.max_socket_datagram_queue_len",
	maxFilesystemBytes: "resource.max_filesystem_bytes",
	maxInodeCount: "resource.max_inode_count",
	maxBlockingReadMs: "resource.max_blocking_read_ms",
	maxPreadBytes: "resource.max_pread_bytes",
	maxFdWriteBytes: "resource.max_fd_write_bytes",
	maxProcessArgvBytes: "resource.max_process_argv_bytes",
	maxProcessEnvBytes: "resource.max_process_env_bytes",
	maxReaddirEntries: "resource.max_readdir_entries",
	maxWasmFuel: "resource.max_wasm_fuel",
	maxWasmMemoryBytes: "resource.max_wasm_memory_bytes",
	maxWasmStackBytes: "resource.max_wasm_stack_bytes",
};

const HTTP_KEYS: Record<keyof NonNullable<AgentOsLimits["http"]>, string> = {
	maxFetchResponseBytes: "limits.http.max_fetch_response_bytes",
};

const TOOLS_KEYS: Record<keyof NonNullable<AgentOsLimits["tools"]>, string> = {
	defaultToolTimeoutMs: "limits.tools.default_tool_timeout_ms",
	maxToolTimeoutMs: "limits.tools.max_tool_timeout_ms",
	maxRegisteredToolkits: "limits.tools.max_registered_toolkits",
	maxRegisteredToolsPerVm: "limits.tools.max_registered_tools_per_vm",
	maxToolsPerToolkit: "limits.tools.max_tools_per_toolkit",
	maxToolSchemaBytes: "limits.tools.max_tool_schema_bytes",
	maxToolExamplesPerTool: "limits.tools.max_tool_examples_per_tool",
	maxToolExampleInputBytes: "limits.tools.max_tool_example_input_bytes",
};

const PLUGINS_KEYS: Record<
	keyof NonNullable<AgentOsLimits["plugins"]>,
	string
> = {
	maxPersistedManifestBytes: "limits.plugins.max_persisted_manifest_bytes",
	maxPersistedManifestFileBytes:
		"limits.plugins.max_persisted_manifest_file_bytes",
};

const ACP_KEYS: Record<keyof NonNullable<AgentOsLimits["acp"]>, string> = {
	maxReadLineBytes: "limits.acp.max_read_line_bytes",
	stdoutBufferByteLimit: "limits.acp.stdout_buffer_byte_limit",
};

const JS_RUNTIME_KEYS: Record<
	keyof NonNullable<AgentOsLimits["jsRuntime"]>,
	string
> = {
	v8HeapLimitMb: "limits.js_runtime.v8_heap_limit_mb",
	capturedOutputLimitBytes: "limits.js_runtime.captured_output_limit_bytes",
	stdinBufferLimitBytes: "limits.js_runtime.stdin_buffer_limit_bytes",
	eventPayloadLimitBytes: "limits.js_runtime.event_payload_limit_bytes",
	v8IpcMaxFrameBytes: "limits.js_runtime.v8_ipc_max_frame_bytes",
};

const PYTHON_KEYS: Record<keyof NonNullable<AgentOsLimits["python"]>, string> = {
	outputBufferMaxBytes: "limits.python.output_buffer_max_bytes",
	executionTimeoutMs: "limits.python.execution_timeout_ms",
	vfsRpcTimeoutMs: "limits.python.vfs_rpc_timeout_ms",
};

const WASM_KEYS: Record<keyof NonNullable<AgentOsLimits["wasm"]>, string> = {
	maxModuleFileBytes: "limits.wasm.max_module_file_bytes",
	capturedOutputLimitBytes: "limits.wasm.captured_output_limit_bytes",
	syncReadLimitBytes: "limits.wasm.sync_read_limit_bytes",
};

function serializeGroup(
	group: Record<string, number | undefined> | undefined,
	keyMap: Record<string, string>,
	groupLabel: string,
	out: Record<string, string>,
): void {
	if (!group) {
		return;
	}
	for (const [field, value] of Object.entries(group)) {
		if (value === undefined) {
			continue;
		}
		const metadataKey = keyMap[field];
		if (metadataKey === undefined) {
			throw new AgentOsLimitsError(
				`unknown limit field ${groupLabel}.${field}`,
			);
		}
		if (
			typeof value !== "number" ||
			!Number.isInteger(value) ||
			value < 0 ||
			!Number.isFinite(value)
		) {
			throw new AgentOsLimitsError(
				`limit ${groupLabel}.${field} must be a non-negative integer, got ${String(value)}`,
			);
		}
		out[metadataKey] = String(value);
	}
}

export function serializeLimitsForSidecar(
	limits: AgentOsLimits | undefined,
): Record<string, string> {
	const out: Record<string, string> = {};
	if (!limits) {
		return out;
	}
	serializeGroup(limits.resources, RESOURCE_KEYS, "resources", out);
	serializeGroup(limits.http, HTTP_KEYS, "http", out);
	serializeGroup(limits.tools, TOOLS_KEYS, "tools", out);
	serializeGroup(limits.plugins, PLUGINS_KEYS, "plugins", out);
	serializeGroup(limits.acp, ACP_KEYS, "acp", out);
	serializeGroup(limits.jsRuntime, JS_RUNTIME_KEYS, "jsRuntime", out);
	serializeGroup(limits.python, PYTHON_KEYS, "python", out);
	serializeGroup(limits.wasm, WASM_KEYS, "wasm", out);
	return out;
}
