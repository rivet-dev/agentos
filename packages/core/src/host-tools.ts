import type { ZodType } from "zod";

/** Maximum length for binding descriptions (characters). */
export const MAX_TOOL_DESCRIPTION_LENGTH = 200;

/**
 * A single binding that executes on the host.
 * Mirrors the shape of AI SDK's tool() but with host-execution semantics.
 */
export interface HostTool<INPUT = any, OUTPUT = any> {
	/** Description shown to the agent in --help and prompt docs. Max 200 characters. */
	description: string;
	/** Zod schema for the input. Drives CLI flag generation and validation. */
	inputSchema: ZodType<INPUT>;
	/** Runs on the host when the agent invokes the binding. */
	execute: (input: INPUT) => Promise<OUTPUT> | OUTPUT;
	/** Examples included in auto-generated prompt docs. */
	examples?: ToolExample<INPUT>[];
	/** Timeout in ms. Default: 30000. */
	timeout?: number;
}

export interface ToolExample<INPUT = any> {
	/** Human description of what this example does. */
	description: string;
	/** The input args for the example. */
	input: INPUT;
}

export type Binding<INPUT = any, OUTPUT = any> = HostTool<INPUT, OUTPUT>;

/**
 * @deprecated Use BindingGroup.
 */
export interface ToolKit {
	/** Toolkit name. Must be lowercase alphanumeric + hyphens. Becomes the CLI suffix: agentos-{name}. */
	name: string;
	/** Description shown in `agentos list-tools` and prompt docs. */
	description: string;
	/** The tools in this toolkit. Keys become subcommands. */
	tools: Record<string, HostTool>;
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

export type BindingGroupInput = BindingGroup | ToolKit;

/** Helper to create a HostTool with type inference. */
export function hostTool<INPUT, OUTPUT>(
	def: HostTool<INPUT, OUTPUT>,
): HostTool<INPUT, OUTPUT> {
	return def;
}

/** Helper to create a binding with type inference. */
export function binding<INPUT, OUTPUT>(
	def: Binding<INPUT, OUTPUT>,
): Binding<INPUT, OUTPUT> {
	return def;
}

/** Helper to create a ToolKit. */
export function toolKit(def: ToolKit): ToolKit {
	return def;
}

/** Helper to create a binding group. */
export function bindingGroup(def: BindingGroup): BindingGroup {
	return def;
}

export function normalizeBindingGroup(group: BindingGroupInput): ToolKit {
	if ("bindings" in group) {
		return {
			name: group.name,
			description: group.description,
			tools: group.bindings,
		};
	}
	return group;
}

export function normalizeBindingGroups(groups: BindingGroupInput[]): ToolKit[] {
	return groups.map(normalizeBindingGroup);
}

const TOOLKIT_COMMAND_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function validateToolCommandName(
	kind: "Binding group" | "Binding",
	name: string,
): void {
	if (TOOLKIT_COMMAND_NAME_RE.test(name)) {
		return;
	}
	throw new Error(
		`${kind} name "${name}" must be lowercase alphanumeric with optional single hyphen separators`,
	);
}

/**
 * Validate all description lengths in the given binding groups.
 * Throws if any group or binding description exceeds MAX_TOOL_DESCRIPTION_LENGTH.
 */
export function validateBindings(bindingGroups: BindingGroupInput[]): void {
	for (const input of bindingGroups) {
		const tk = normalizeBindingGroup(input);
		validateToolCommandName("Binding group", tk.name);
		if (tk.description.length > MAX_TOOL_DESCRIPTION_LENGTH) {
			throw new Error(
				`Binding group "${tk.name}" description is ${tk.description.length} characters, max is ${MAX_TOOL_DESCRIPTION_LENGTH}`,
			);
		}
		for (const [toolName, tool] of Object.entries(tk.tools)) {
			validateToolCommandName("Binding", toolName);
			if (tool.description.length > MAX_TOOL_DESCRIPTION_LENGTH) {
				throw new Error(
					`Binding "${tk.name}/${toolName}" description is ${tool.description.length} characters, max is ${MAX_TOOL_DESCRIPTION_LENGTH}`,
				);
			}
		}
	}
}

/** @deprecated Use validateBindings. */
export const validateToolkits = validateBindings;
