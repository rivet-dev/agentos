import type { ZodType } from "zod";

/** Maximum length for binding descriptions (characters). */
export const MAX_BINDING_DESCRIPTION_LENGTH = 200;

/**
 * A single binding that executes on the host.
 * Uses the same typed input/execute pattern as common agent SDK helpers, but
 * with host-execution semantics.
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
 * A named group of bindings. Becomes a CLI binary: agentos-{name}.
 */
export interface BindingGroup {
	/** Binding group name. Must be lowercase alphanumeric + hyphens. */
	name: string;
	/** Description shown in binding listings and prompt docs. */
	description: string;
	/** The bindings in this group. Keys become subcommands. */
	bindings: Record<string, Binding>;
}

export type BindingGroupInput = BindingGroup;

/** Helper to create a binding with type inference. */
export function binding<INPUT, OUTPUT>(
	def: Binding<INPUT, OUTPUT>,
): Binding<INPUT, OUTPUT> {
	return def;
}

/** Helper to create a binding group. */
export function bindingGroup(def: BindingGroup): BindingGroup {
	return def;
}

export function normalizeBindingGroup(group: BindingGroupInput): BindingGroup {
	return group;
}

export function normalizeBindingGroups(
	groups: BindingGroupInput[],
): BindingGroup[] {
	return groups.map(normalizeBindingGroup);
}

const BINDING_COMMAND_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function validateBindingCommandName(
	kind: "Binding group" | "Binding",
	name: string,
): void {
	if (BINDING_COMMAND_NAME_RE.test(name)) {
		return;
	}
	throw new Error(
		`${kind} name "${name}" must be lowercase alphanumeric with optional single hyphen separators`,
	);
}

/**
 * Validate all description lengths in the given binding groups.
 * Throws if any group or binding description exceeds MAX_BINDING_DESCRIPTION_LENGTH.
 */
export function validateBindings(bindingGroups: BindingGroupInput[]): void {
	for (const input of bindingGroups) {
		const group = normalizeBindingGroup(input);
		validateBindingCommandName("Binding group", group.name);
		if (group.description.length > MAX_BINDING_DESCRIPTION_LENGTH) {
			throw new Error(
				`Binding group "${group.name}" description is ${group.description.length} characters, max is ${MAX_BINDING_DESCRIPTION_LENGTH}`,
			);
		}
		for (const [bindingName, bindingDefinition] of Object.entries(
			group.bindings,
		)) {
			validateBindingCommandName("Binding", bindingName);
			if (
				bindingDefinition.description.length > MAX_BINDING_DESCRIPTION_LENGTH
			) {
				throw new Error(
					`Binding "${group.name}/${bindingName}" description is ${bindingDefinition.description.length} characters, max is ${MAX_BINDING_DESCRIPTION_LENGTH}`,
				);
			}
		}
	}
}
