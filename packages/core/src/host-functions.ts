import type { ZodType } from "zod";

/** Maximum length for host function descriptions (characters). */
export const MAX_HOST_FUNCTION_DESCRIPTION_LENGTH = 200;

/**
 * A single function that executes on the host.
 */
export interface HostFunction<INPUT = any, OUTPUT = any> {
	/** Description shown to the agent in --help and prompt docs. Max 200 characters. */
	description: string;
	/** Zod schema for the input. Drives CLI flag generation and validation. */
	inputSchema: ZodType<INPUT>;
	/** Runs on the host when the agent invokes the function. */
	execute: (input: INPUT) => Promise<OUTPUT> | OUTPUT;
	/** Examples included in auto-generated prompt docs. */
	examples?: HostFunctionExample<INPUT>[];
	/** Timeout in ms. Default: 30000. */
	timeout?: number;
}

export interface HostFunctionExample<INPUT = any> {
	/** Human description of what this example does. */
	description: string;
	/** The input args for the example. */
	input: INPUT;
}

/**
 * A named collection of host functions. Becomes a CLI binary: agentos-{name}.
 */
export interface HostFunctions {
	/** Collection name. Must be lowercase alphanumeric + hyphens. Becomes the CLI suffix: agentos-{name}. */
	name: string;
	/** Description shown in `agentos list-host-functions` and prompt docs. */
	description: string;
	/** The functions in this collection. Keys become subcommands. */
	functions: Record<string, HostFunction>;
}

/** Helper to create a host function with type inference. */
export function hostFunction<INPUT, OUTPUT>(
	def: HostFunction<INPUT, OUTPUT>,
): HostFunction<INPUT, OUTPUT> {
	return def;
}

/** Helper to create a named host function collection. */
export function hostFunctions(def: HostFunctions): HostFunctions {
	return def;
}

const HOST_FUNCTION_COMMAND_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function validateHostFunctionCommandName(kind: string, name: string): void {
	if (HOST_FUNCTION_COMMAND_NAME_RE.test(name)) {
		return;
	}
	throw new Error(
		`${kind} name "${name}" must be lowercase alphanumeric with optional single hyphen separators`,
	);
}

/**
 * Validate every host function collection and function.
 */
export function validateHostFunctions(collections: HostFunctions[]): void {
	for (const collection of collections) {
		validateHostFunctionCommandName("Host function collection", collection.name);
		if (collection.description.length > MAX_HOST_FUNCTION_DESCRIPTION_LENGTH) {
			throw new Error(
				`Host function collection "${collection.name}" description is ${collection.description.length} characters, max is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH}`,
			);
		}
		for (const [functionName, definition] of Object.entries(
			collection.functions,
		)) {
			validateHostFunctionCommandName("Host function", functionName);
			if (definition.description.length > MAX_HOST_FUNCTION_DESCRIPTION_LENGTH) {
				throw new Error(
					`Host function "${collection.name}/${functionName}" description is ${definition.description.length} characters, max is ${MAX_HOST_FUNCTION_DESCRIPTION_LENGTH}`,
				);
			}
		}
	}
}
