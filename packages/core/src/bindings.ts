import type { ZodType } from "zod";

/** Maximum length for binding descriptions (characters). */
export const MAX_BINDING_DESCRIPTION_LENGTH = 200;

/**
 * A single binding that executes on the host.
 */
export interface Binding<INPUT = any, OUTPUT = any> {
	/** Description shown to the agent in --help and prompt docs. Max 200 characters. */
	description: string;
	/** Zod schema for the input. Drives CLI flag generation and validation. */
	inputSchema: ZodType<INPUT>;
	/** Runs on the host when the agent invokes the binding. */
	execute: (input: INPUT) => Promise<OUTPUT> | OUTPUT;
	/** Examples included in auto-generated prompt docs. */
	examples?: BindingExample<INPUT>[];
	/** Timeout in ms. Default: 30000. */
	timeout?: number;
}

export interface BindingExample<INPUT = any> {
	/** Human description of what this example does. */
	description: string;
	/** The input args for the example. */
	input: INPUT;
}

/**
 * A named collection of bindings. Becomes a CLI binary: agentos-{name}.
 */
export interface Bindings {
	/** Collection name. Must be lowercase alphanumeric + hyphens. Becomes the CLI suffix: agentos-{name}. */
	name: string;
	/** Description shown in `agentos list-bindings` and prompt docs. */
	description: string;
	/** The bindings in this collection. Keys become subcommands. */
	bindings: Record<string, Binding>;
}

/** Helper to create a binding with type inference. */
export function binding<INPUT, OUTPUT>(
	def: Binding<INPUT, OUTPUT>,
): Binding<INPUT, OUTPUT> {
	return def;
}

/** Helper to create a named binding collection. */
export function bindings(def: Bindings): Bindings {
	return def;
}

const BINDING_COMMAND_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function validateBindingCommandName(kind: string, name: string): void {
	if (BINDING_COMMAND_NAME_RE.test(name)) {
		return;
	}
	throw new Error(
		`${kind} name "${name}" must be lowercase alphanumeric with optional single hyphen separators`,
	);
}

/**
 * Validate every binding collection and binding.
 */
export function validateBindings(collections: Bindings[]): void {
	for (const collection of collections) {
		validateBindingCommandName("Binding collection", collection.name);
		if (collection.description.length > MAX_BINDING_DESCRIPTION_LENGTH) {
			throw new Error(
				`Binding collection "${collection.name}" description is ${collection.description.length} characters, max is ${MAX_BINDING_DESCRIPTION_LENGTH}`,
			);
		}
		for (const [bindingName, definition] of Object.entries(
			collection.bindings,
		)) {
			validateBindingCommandName("Binding", bindingName);
			if (definition.description.length > MAX_BINDING_DESCRIPTION_LENGTH) {
				throw new Error(
					`Binding "${collection.name}/${bindingName}" description is ${definition.description.length} characters, max is ${MAX_BINDING_DESCRIPTION_LENGTH}`,
				);
			}
		}
	}
}
