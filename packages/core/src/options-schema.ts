import { z } from "zod/v4";
import type {
	AgentOsOptions,
	AgentStderrHandler,
	NativeMountConfig,
} from "./agent-os.js";
import type { HostTool, ToolKit } from "./host-tools.js";

const stringArray = z.array(z.string());
const nonNegativeInteger = z.number().int().nonnegative();
const positiveInteger = z.number().int().positive();
const functionSchema = z.custom<(...args: any[]) => any>(
	(value) => typeof value === "function",
	{ message: "Expected function" },
);

const permissionModeSchema = z.enum(["allow", "deny"]);

const fsPermissionRuleSchema = z
	.object({
		mode: permissionModeSchema,
		operations: stringArray.optional(),
		paths: stringArray.optional(),
	})
	.strict();

const patternPermissionRuleSchema = z
	.object({
		mode: permissionModeSchema,
		operations: stringArray.optional(),
		patterns: stringArray.optional(),
	})
	.strict();

const fsRulePermissionsSchema = z
	.object({
		default: permissionModeSchema.optional(),
		rules: z.array(fsPermissionRuleSchema),
	})
	.strict();

const patternRulePermissionsSchema = z
	.object({
		default: permissionModeSchema.optional(),
		rules: z.array(patternPermissionRuleSchema),
	})
	.strict();

const fsPermissionsSchema = z.union([permissionModeSchema, fsRulePermissionsSchema]);
const patternPermissionsSchema = z.union([
	permissionModeSchema,
	patternRulePermissionsSchema,
]);

export const permissionsSchema = z
	.object({
		fs: fsPermissionsSchema.optional(),
		network: patternPermissionsSchema.optional(),
		childProcess: patternPermissionsSchema.optional(),
		process: patternPermissionsSchema.optional(),
		env: patternPermissionsSchema.optional(),
		binding: patternPermissionsSchema.optional(),
	})
	.strict();

export const agentOsLimitsSchema = z
	.object({
		resources: z
			.object({
				cpuCount: positiveInteger.optional(),
				maxProcesses: nonNegativeInteger.optional(),
				maxOpenFds: nonNegativeInteger.optional(),
				maxPipes: nonNegativeInteger.optional(),
				maxPtys: nonNegativeInteger.optional(),
				maxSockets: nonNegativeInteger.optional(),
				maxConnections: nonNegativeInteger.optional(),
				maxSocketBufferedBytes: nonNegativeInteger.optional(),
				maxSocketDatagramQueueLen: nonNegativeInteger.optional(),
				maxFilesystemBytes: nonNegativeInteger.optional(),
				maxInodeCount: nonNegativeInteger.optional(),
				maxBlockingReadMs: nonNegativeInteger.optional(),
				maxPreadBytes: nonNegativeInteger.optional(),
				maxFdWriteBytes: nonNegativeInteger.optional(),
				maxProcessArgvBytes: nonNegativeInteger.optional(),
				maxProcessEnvBytes: nonNegativeInteger.optional(),
				maxReaddirEntries: nonNegativeInteger.optional(),
				maxWasmFuel: nonNegativeInteger.optional(),
				maxWasmMemoryBytes: nonNegativeInteger.optional(),
				maxWasmStackBytes: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		http: z
			.object({ maxFetchResponseBytes: nonNegativeInteger.optional() })
			.strict()
			.optional(),
		tools: z
			.object({
				defaultToolTimeoutMs: nonNegativeInteger.optional(),
				maxToolTimeoutMs: nonNegativeInteger.optional(),
				maxRegisteredToolkits: nonNegativeInteger.optional(),
				maxRegisteredToolsPerVm: nonNegativeInteger.optional(),
				maxToolsPerToolkit: nonNegativeInteger.optional(),
				maxToolSchemaBytes: nonNegativeInteger.optional(),
				maxToolExamplesPerTool: nonNegativeInteger.optional(),
				maxToolExampleInputBytes: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		plugins: z
			.object({
				maxPersistedManifestBytes: nonNegativeInteger.optional(),
				maxPersistedManifestFileBytes: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		acp: z
			.object({
				maxReadLineBytes: nonNegativeInteger.optional(),
				stdoutBufferByteLimit: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		jsRuntime: z
			.object({
				v8HeapLimitMb: nonNegativeInteger.optional(),
				capturedOutputLimitBytes: nonNegativeInteger.optional(),
				stdinBufferLimitBytes: nonNegativeInteger.optional(),
				eventPayloadLimitBytes: nonNegativeInteger.optional(),
				v8IpcMaxFrameBytes: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		python: z
			.object({
				outputBufferMaxBytes: nonNegativeInteger.optional(),
				executionTimeoutMs: nonNegativeInteger.optional(),
				vfsRpcTimeoutMs: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
		wasm: z
			.object({
				maxModuleFileBytes: nonNegativeInteger.optional(),
				capturedOutputLimitBytes: nonNegativeInteger.optional(),
				syncReadLimitBytes: nonNegativeInteger.optional(),
			})
			.strict()
			.optional(),
	})
	.strict();

const rootLowerInputSchema = z.union([
	z.object({ kind: z.literal("bundled-base-filesystem") }).strict(),
	z.object({ kind: z.literal("snapshot-export"), source: z.unknown() }).strict(),
]);

export const rootFilesystemConfigSchema = z
	.object({
		type: z.literal("overlay").optional(),
		mode: z.enum(["ephemeral", "read-only"]).optional(),
		disableDefaultBaseLayer: z.boolean().optional(),
		lowers: z.array(rootLowerInputSchema).optional(),
	})
	.strict();

const nativeMountPluginSchema = z
	.object({
		id: z.string(),
		config: z.unknown().optional(),
	})
	.strict();

const plainMountConfigSchema = z
	.object({
		path: z.string(),
		driver: z.custom((value) => typeof value === "object" && value !== null, {
			message: "Expected filesystem driver object",
		}),
		readOnly: z.boolean().optional(),
	})
	.strict();

export const nativeMountConfigSchema = z
	.object({
		path: z.string(),
		plugin: nativeMountPluginSchema,
		readOnly: z.boolean().optional(),
	})
	.strict() as z.ZodType<NativeMountConfig>;

const overlayMountConfigSchema = z
	.object({
		path: z.string(),
		filesystem: z
			.object({
				type: z.literal("overlay"),
				store: z.unknown(),
				mode: z.enum(["ephemeral", "read-only"]).optional(),
				lowers: z.array(z.unknown()),
			})
			.strict(),
	})
	.strict();

export const mountConfigSchema = z.union([
	plainMountConfigSchema,
	nativeMountConfigSchema,
	overlayMountConfigSchema,
]);

export const sharedSidecarConfigSchema = z
	.object({
		kind: z.literal("shared"),
		pool: z.string().optional(),
	})
	.strict();

const explicitSidecarSchema = z
	.object({
		kind: z.literal("explicit"),
		handle: z.unknown(),
	})
	.strict();

export const sidecarConfigSchema = z.union([
	sharedSidecarConfigSchema,
	explicitSidecarSchema,
]);

const toolExampleSchema = z
	.object({
		description: z.string(),
		input: z.unknown(),
	})
	.strict();

export const hostToolSchema = z
	.object({
		description: z.string(),
		inputSchema: z.custom((value) => typeof value === "object" && value !== null, {
			message: "Expected Zod schema object",
		}),
		execute: functionSchema,
		examples: z.array(toolExampleSchema).optional(),
		timeout: nonNegativeInteger.optional(),
	})
	.strict() as z.ZodType<HostTool>;

export const toolKitSchema = z
	.object({
		name: z.string(),
		description: z.string(),
		tools: z.record(z.string(), hostToolSchema),
	})
	.strict() as z.ZodType<ToolKit>;

/**
 * Shared AgentOsOptions field schemas.
 *
 * Core uses the full object. The Rivet/native actor composes a narrower
 * native-safe subset in `packages/agentos/src/config.ts`; keep that subset and
 * `crates/agentos-actor-plugin/src/config.rs::AgentOsConfigJson` aligned when
 * adding options that cross the native boundary.
 */
export const agentOsOptionFieldSchemas = {
	software: z.array(z.unknown()).optional(),
	defaultSoftware: z.boolean().optional(),
	loopbackExemptPorts: z.array(z.number().int().min(0).max(65535)).optional(),
	allowedNodeBuiltins: stringArray.optional(),
	rootFilesystem: rootFilesystemConfigSchema.optional(),
	mounts: z.array(mountConfigSchema).optional(),
	moduleAccessCwd: z.string().optional(),
	additionalInstructions: z.string().optional(),
	scheduleDriver: z
		.custom((value) => typeof value === "object" && value !== null, {
			message: "Expected schedule driver object",
		})
		.optional(),
	toolKits: z.array(toolKitSchema).optional(),
	permissions: permissionsSchema.optional(),
	sidecar: sidecarConfigSchema.optional(),
	limits: agentOsLimitsSchema.optional(),
	onAgentStderr: z.custom<AgentStderrHandler>(
		(value) => typeof value === "function",
		{ message: "Expected function" },
	).optional(),
} as const;

export const agentOsOptionsSchema = z
	.object(agentOsOptionFieldSchemas)
	.strict() as z.ZodType<AgentOsOptions>;

export function parseAgentOsOptions(options?: AgentOsOptions): AgentOsOptions {
	return agentOsOptionsSchema.parse(options ?? {});
}
