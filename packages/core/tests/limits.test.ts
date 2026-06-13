import { describe, expect, test } from "vitest";
import type { AgentOsLimits } from "../src/agent-os.js";
import {
	AgentOsLimitsError,
	serializeLimitsForSidecar,
} from "../src/sidecar/limits.js";

describe("serializeLimitsForSidecar", () => {
	test("returns no entries when limits are unset", () => {
		expect(serializeLimitsForSidecar(undefined)).toEqual({});
		expect(serializeLimitsForSidecar({})).toEqual({});
	});

	test("maps kernel resources to existing resource.* keys", () => {
		const limits: AgentOsLimits = {
			resources: { maxProcesses: 8, maxFilesystemBytes: 1024 },
		};
		expect(serializeLimitsForSidecar(limits)).toEqual({
			"resource.max_processes": "8",
			"resource.max_filesystem_bytes": "1024",
		});
	});

	test("maps other groups to limits.<group>.<field> snake_case keys", () => {
		const limits: AgentOsLimits = {
			http: { maxFetchResponseBytes: 65536 },
			tools: { maxToolSchemaBytes: 4096, defaultToolTimeoutMs: 1000 },
			plugins: { maxPersistedManifestFileBytes: 2048 },
			acp: { maxReadLineBytes: 8192 },
			jsRuntime: { v8HeapLimitMb: 256, v8IpcMaxFrameBytes: 1048576 },
			python: { executionTimeoutMs: 5000 },
			wasm: { maxModuleFileBytes: 1024, syncReadLimitBytes: 512 },
		};
		expect(serializeLimitsForSidecar(limits)).toEqual({
			"limits.http.max_fetch_response_bytes": "65536",
			"limits.tools.max_tool_schema_bytes": "4096",
			"limits.tools.default_tool_timeout_ms": "1000",
			"limits.plugins.max_persisted_manifest_file_bytes": "2048",
			"limits.acp.max_read_line_bytes": "8192",
			"limits.js_runtime.v8_heap_limit_mb": "256",
			"limits.js_runtime.v8_ipc_max_frame_bytes": "1048576",
			"limits.python.execution_timeout_ms": "5000",
			"limits.wasm.max_module_file_bytes": "1024",
			"limits.wasm.sync_read_limit_bytes": "512",
		});
	});

	test("omits undefined fields", () => {
		const limits: AgentOsLimits = {
			tools: { maxToolSchemaBytes: 4096, maxToolTimeoutMs: undefined },
		};
		expect(serializeLimitsForSidecar(limits)).toEqual({
			"limits.tools.max_tool_schema_bytes": "4096",
		});
	});

	test("throws on negative values", () => {
		expect(() =>
			serializeLimitsForSidecar({ resources: { maxProcesses: -1 } }),
		).toThrow(AgentOsLimitsError);
	});

	test("throws on non-integer values", () => {
		expect(() =>
			serializeLimitsForSidecar({ http: { maxFetchResponseBytes: 1.5 } }),
		).toThrow(AgentOsLimitsError);
	});

	test("throws on non-finite values", () => {
		expect(() =>
			serializeLimitsForSidecar({
				wasm: { maxModuleFileBytes: Number.POSITIVE_INFINITY },
			}),
		).toThrow(AgentOsLimitsError);
	});
});
