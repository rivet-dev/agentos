import type { ZodType, z } from "zod";
import { schemaDescription } from "./host-functions-zod.js";

/**
 * A single function that executes on the host.
 *
 * The function carries no name or description of its own: the key it is
 * registered under names it, and `inputSchema.describe()` documents it for the
 * agent.
 */
export interface HostFunction<SCHEMA extends ZodType = ZodType, OUTPUT = any> {
	/**
	 * Zod schema for the input. Drives CLI flag generation and validation.
	 * `.describe()` on the schema becomes the function's description in `--help`
	 * and in the agent's system prompt; `.describe()` on a field documents that
	 * field's flag.
	 */
	inputSchema: SCHEMA;
	/**
	 * Runs on the host when the agent invokes the function. Its input is the
	 * type `inputSchema` describes, inferred when the collection is written
	 * inline in the call that takes it.
	 */
	execute: (input: HostFunctionInput<SCHEMA>) => Promise<OUTPUT> | OUTPUT;
	/** Examples included in auto-generated prompt docs. */
	examples?: HostFunctionExample<HostFunctionInput<SCHEMA>>[];
	/** Timeout in ms. Default: 30000. */
	timeout?: number;
}

/**
 * The input type a function's `execute` receives. A concrete schema gives the
 * type it describes; a collection built outside the call that takes it has no
 * schema to infer from, so it falls back to `any` rather than making every
 * handler annotate its parameter.
 */
export type HostFunctionInput<SCHEMA extends ZodType> = ZodType extends SCHEMA
	? any
	: z.infer<SCHEMA>;

export interface HostFunctionExample<INPUT = any> {
	/** Human description of what this example does. */
	description: string;
	/** The input args for the example. */
	input: INPUT;
}

/**
 * One collection of host functions, keyed by function name. Each key becomes a
 * subcommand of the collection's CLI binary and a method on the collection's
 * guest global.
 */
export type HostFunctionCollection<
	T extends Record<string, ZodType> = Record<string, ZodType>,
> = { [name in keyof T]: HostFunction<T[name]> };

/**
 * The input schemas behind a set of collections. Only used to give each
 * `execute` its input type: `AgentOs.create()` infers this from the literal, so
 * `execute` sees the type its own `inputSchema` describes without a wrapper
 * call to hang the inference on.
 */
export type HostFunctionSchemas = Record<string, Record<string, ZodType>>;

/**
 * Host function collections, keyed by collection name. Each key becomes the CLI
 * binary `agentos-{name}` and a frozen guest global.
 *
 * ```ts
 * hostFunctions: {
 *   store: {
 *     listOrders: { inputSchema, execute },
 *   },
 * }
 * ```
 */
export type HostFunctionCollections<
	T extends HostFunctionSchemas = HostFunctionSchemas,
> = { [collection in keyof T]: HostFunctionCollection<T[collection]> };

/**
 * A collection resolved to the names the VM uses. Keys arrive as JavaScript
 * identifiers and are converted once here, so the rest of the client and the
 * sidecar only ever see kebab-case command names.
 */
export interface ResolvedHostFunctions {
	/** Kebab-case collection name. Becomes the CLI suffix: `agentos-{name}`. */
	name: string;
	/** Functions keyed by kebab-case command name. */
	functions: Record<string, HostFunction>;
}

const HOST_FUNCTION_COMMAND_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * Convert a registration key to its command name. `listOrders` and
 * `list-orders` both become `list-orders`, so the guest sees one spelling
 * whichever the caller wrote.
 */
export function hostFunctionCommandName(key: string): string {
	return key
		.replace(/([a-z0-9])([A-Z])/g, "$1-$2")
		.replace(/([A-Z]+)([A-Z][a-z])/g, "$1-$2")
		.toLowerCase();
}

function toCommandName(kind: string, key: string): string {
	const name = hostFunctionCommandName(key);
	if (!HOST_FUNCTION_COMMAND_NAME_RE.test(name)) {
		throw new Error(
			`${kind} name "${key}" must be alphanumeric, written in camelCase or with single hyphen separators`,
		);
	}
	return name;
}

/** The description the agent sees, taken from the input schema's `.describe()`. */
export function hostFunctionDescription(definition: HostFunction): string {
	return schemaDescription(definition.inputSchema) ?? "";
}

/**
 * Resolve the caller's collections into the shape the client and sidecar use.
 * Throws on a key that cannot become a command name, and on two keys that
 * collide once converted.
 */
export function resolveHostFunctions(
	collections: HostFunctionCollections,
): ResolvedHostFunctions[] {
	const resolved: ResolvedHostFunctions[] = [];
	const seenCollections = new Map<string, string>();

	for (const [collectionKey, collection] of Object.entries(collections)) {
		const name = toCommandName("Host function collection", collectionKey);
		const collidedCollection = seenCollections.get(name);
		if (collidedCollection !== undefined) {
			throw new Error(
				`Host function collections "${collidedCollection}" and "${collectionKey}" both resolve to the command name "${name}"`,
			);
		}
		seenCollections.set(name, collectionKey);

		const functions: Record<string, HostFunction> = {};
		const seenFunctions = new Map<string, string>();
		for (const [functionKey, definition] of Object.entries(collection)) {
			const functionName = toCommandName("Host function", functionKey);
			const collidedFunction = seenFunctions.get(functionName);
			if (collidedFunction !== undefined) {
				throw new Error(
					`Host functions "${collidedFunction}" and "${functionKey}" in collection "${collectionKey}" both resolve to the command name "${functionName}"`,
				);
			}
			seenFunctions.set(functionName, functionKey);
			functions[functionName] = definition;
		}

		resolved.push({ name, functions });
	}

	return resolved;
}
