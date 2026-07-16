import { z } from "zod";
import type { NodeRuntimeCreateOptions } from "./node-runtime.js";

const permissionModeSchema = z.enum(["allow", "deny"]);
const stringArray = z.array(z.string());

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

export const nodeRuntimePermissionsSchema = z
	.object({
		fs: fsPermissionsSchema.optional(),
		network: patternPermissionsSchema.optional(),
		childProcess: patternPermissionsSchema.optional(),
		process: patternPermissionsSchema.optional(),
		env: patternPermissionsSchema.optional(),
		binding: patternPermissionsSchema.optional(),
	})
	.strict();

const uint8ArraySchema = z.custom<Uint8Array>(
	(value: unknown) => value instanceof Uint8Array,
	{ message: "Expected Uint8Array" },
);

const hostDirectoryMountSchema = z
	.object({
		guestPath: z.string(),
		hostPath: z.string(),
		readOnly: z.boolean().optional(),
	})
	.strict();

const nodeModulesMountSchema = z
	.object({
		hostPath: z.string(),
		guestPath: z.string().optional(),
	})
	.strict();

const jsRuntimeSchema = z
	.object({
		platform: z.enum(["node", "browser", "neutral", "bare"]).optional(),
		moduleResolution: z.enum(["node", "relative", "none"]).optional(),
		allowedBuiltins: stringArray.optional(),
		highResolutionTime: z.boolean().optional(),
	})
	.strict();

const bindingExampleSchema = z
	.object({
		description: z.string(),
		input: z.unknown(),
	})
	.strict();

const bindingDefinitionSchema = z
	.object({
		description: z.string(),
		inputSchema: z.custom<object>(
			(value: unknown) => typeof value === "object" && value !== null,
			{ message: "Expected JSON Schema object" },
		),
		timeoutMs: z.number().int().nonnegative().optional(),
		examples: z.array(bindingExampleSchema).optional(),
		commandAliases: stringArray.optional(),
		handler: z.custom<(input: unknown) => unknown | Promise<unknown>>(
			(value: unknown) => typeof value === "function",
			{ message: "Expected function" },
		),
	})
	.strict();

/**
 * Runtime validation for the public `NodeRuntime.create(...)` API.
 *
 * This is the TS-side guard for the ergonomic options shape. The sidecar VM
 * JSON it eventually produces is still validated by
 * `crates/vm-config/src/lib.rs::CreateVmConfig` with `deny_unknown_fields`.
 * Keep these in sync when adding high-level create options that translate into
 * the Rust VM config.
 */
export const nodeRuntimeCreateOptionsSchema = z
	.object({
		filesystem: z.custom<NodeRuntimeCreateOptions["filesystem"]>(
			(value: unknown) => typeof value === "object" && value !== null,
			{ message: "Expected caller-owned VirtualFileSystem object" },
		),
		env: z.record(z.string(), z.string()).optional(),
		cwd: z.string().optional(),
		permissions: nodeRuntimePermissionsSchema.optional(),
		commandsDir: z.string().optional(),
		wasmCommandDirs: stringArray.optional(),
		sidecar: z
			.custom((value: unknown) => typeof value === "object" && value !== null, {
				message: "Expected SidecarProcess object",
			})
			.optional(),
		onBootTiming: z
			.custom<(timing: unknown) => void>(
				(value: unknown) => typeof value === "function",
				{ message: "Expected function" },
			)
			.optional(),
		files: z
			.record(z.string(), z.union([z.string(), uint8ArraySchema]))
			.optional(),
		mounts: z.array(hostDirectoryMountSchema).optional(),
		nodeModules: z.union([z.string(), nodeModulesMountSchema]).optional(),
		bindings: z.record(z.string(), bindingDefinitionSchema).optional(),
		loopbackExemptPorts: z
			.array(z.number().int().min(0).max(65535))
			.optional(),
		jsRuntime: jsRuntimeSchema.optional(),
	})
	.strict() as z.ZodType<NodeRuntimeCreateOptions>;

export function parseNodeRuntimeCreateOptions(
	options: NodeRuntimeCreateOptions,
): NodeRuntimeCreateOptions {
	return nodeRuntimeCreateOptionsSchema.parse(options);
}
