import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(packageRoot, "../..");
const schemaPath = resolve(packageRoot, "schema/agentos.json");
const outputPath = resolve(workspaceRoot, "packages/agentos/src/generated/contract.ts");

export function readRustContract() {
	const output = execFileSync(
		"cargo",
		["run", "--quiet", "-p", "agentos-bindgen-export"],
		{
			cwd: workspaceRoot,
			encoding: "utf8",
			maxBuffer: 16 * 1024 * 1024,
			stdio: ["ignore", "pipe", "inherit"],
		},
	);
	return validateContract(JSON.parse(output));
}

export function validateContract(contract) {
	assert.equal(contract.schemaVersion, 1, "unsupported contract schema version");
	assert.match(contract.apiVersion, /^agentos-sdk\.dev\/v\d+(alpha\d+)?$/);
	assert.ok(Number.isInteger(contract.contractMajor) && contract.contractMajor > 0);
	assert.match(contract.contractHash, /^sha256:[0-9a-f]{64}$/);
	assert.ok(Array.isArray(contract.capabilities));
	assert.equal(new Set(contract.capabilities).size, contract.capabilities.length);
	assert.equal(contract.actorName, "agentOS", "actor name must remain agentOS");
	assert.ok(Array.isArray(contract.actions));
	assert.ok(Array.isArray(contract.events));
	assert.ok(Array.isArray(contract.types));

	const publicActions = contract.actions.filter((action) => action.public);
	assert.ok(
		contract.actions
			.filter((action) => !action.public)
			.every((action) => action.name.startsWith("__")),
		"only reserved actions may be internal",
	);
	validateDottedNames(publicActions.map((action) => action.name));
	validateDottedNames(contract.events.map((event) => event.name));
	assert.equal(
		new Set(contract.types.map((type) => type.name)).size,
		contract.types.length,
		"Rust DTOs must have unique TypeScript names",
	);
	return { ...contract, publicActions };
}

export function validateDottedNames(names) {
	const paths = new Set();
	for (const name of names) {
		assert.match(name, /^[A-Za-z_$][\w$]*(\.[A-Za-z_$][\w$]*)+$/);
		assert.ok(!paths.has(name), `duplicate contract name: ${name}`);
		const segments = name.split(".");
		for (let index = 1; index < segments.length; index += 1) {
			const parent = segments.slice(0, index).join(".");
			assert.ok(!paths.has(parent), `${parent} is both an action and a group`);
		}
		for (const existing of paths) {
			assert.ok(!existing.startsWith(`${name}.`), `${name} is both an action and a group`);
		}
		paths.add(name);
	}
}

function optionalizeNullableFields(declaration) {
	return declaration.replace(
		/([A-Za-z_$][\w$]*|"[^"]+"): ([^,\n]+?) \| null,/g,
		"$1?: $2 | null,",
	);
}

function typeDeclaration(declaration, direction) {
	const normalized =
		direction === "input"
			? optionalizeNullableFields(
					declaration.replaceAll("bigint", "number | bigint"),
				)
			: optionalizeNullableFields(
					declaration.replaceAll("bigint", "number | bigint"),
				);
	return normalized.replace(/^type /, "export type ");
}

function qualifyShape(shape, namespace, names) {
	let qualified = shape;
	for (const name of names) {
		qualified = qualified.replace(
			new RegExp(`\\b${escapeRegExp(name)}\\b`, "g"),
			`${namespace}.${name}`,
		);
	}
	return qualified;
}

function escapeRegExp(value) {
	return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function actionTypeName(name) {
	return name
		.split(".")
		.map((segment) => {
			if (segment === "javascript") return "JavaScript";
			if (segment === "typescript") return "TypeScript";
			return `${segment[0].toUpperCase()}${segment.slice(1)}`;
		})
		.join("");
}

function buildTree(actions) {
	const root = { children: new Map(), action: undefined };
	for (const action of actions) {
		let node = root;
		for (const segment of action.name.split(".")) {
			let child = node.children.get(segment);
			if (!child) {
				child = { children: new Map(), action: undefined };
				node.children.set(segment, child);
			}
			node = child;
		}
		node.action = action;
	}
	return root;
}

function renderActionTree(node, depth = 1) {
	const indent = "\t".repeat(depth);
	const lines = ["{"];
	for (const [segment, child] of node.children) {
		if (child.action) {
			const typeName = actionTypeName(child.action.name);
			const args =
				child.action.input === "null"
					? ""
					: `input: ${typeName}Input`;
			lines.push(
				`${indent}readonly ${segment}: (${args}) => Promise<${typeName}Output>;`,
			);
		} else {
			lines.push(
				`${indent}readonly ${segment}: ${renderActionTree(child, depth + 1)};`,
			);
		}
	}
	lines.push(`${"\t".repeat(depth - 1)}}`);
	return lines.join("\n");
}

export function emitTypeScript(contract) {
	const names = contract.types.map((type) => type.name).sort((a, b) => b.length - a.length);
	const lines = [
		"// Generated by @rivet-dev/agentos-bindgen. Do not edit.",
		'import type {',
		"\tBaseActorDefinition,",
		"\tClient,",
		"\tRegistry,",
		"\tType,",
		'} from "rivetkit";',
		'import { createClient } from "rivetkit/client";',
		'import type {',
		"\tActorAccessor,",
		"\tActorConnectOptions,",
		"\tActorConn,",
		"\tActorHandle,",
		"\tClientConfigInput,",
		"\tCreateOptions,",
		"\tGetOptions,",
		"\tGetWithIdOptions,",
		'} from "rivetkit/client";',
		"",
		`export const AGENTOS_API_VERSION = ${JSON.stringify(contract.apiVersion)} as const;`,
		`export const AGENTOS_CONTRACT_MAJOR = ${contract.contractMajor} as const;`,
		`export const AGENTOS_CONTRACT_HASH = ${JSON.stringify(contract.contractHash)} as const;`,
		`export const AGENTOS_CAPABILITIES = ${JSON.stringify(contract.capabilities)} as const;`,
		"export const AGENTOS_CONTRACT = {",
		"\tapiVersion: AGENTOS_API_VERSION,",
		"\tcontractMajor: AGENTOS_CONTRACT_MAJOR,",
		"\tcontractHash: AGENTOS_CONTRACT_HASH,",
		"\tcapabilities: AGENTOS_CAPABILITIES,",
		"} as const;",
		"",
		"export namespace Input {",
		...contract.types.map((type) => `\t${typeDeclaration(type.declaration, "input")}`),
		"}",
		"",
		"export namespace Output {",
		...contract.types.map((type) => `\t${typeDeclaration(type.declaration, "output")}`),
		"}",
		"",
		"export type AgentOsActorCreateInput = Input.AgentOsActorCreateInput;",
		"export type AgentOsRivetError = {",
		"\tgroup: string;",
		"\tcode: string;",
		"\tmessage: string;",
		"\tmetadata: Output.JsonValue | null;",
		"};",
		"",
	];

	for (const action of contract.publicActions) {
		const name = actionTypeName(action.name);
		lines.push(
			`export type ${name}Input = ${qualifyShape(optionalizeNullableFields(action.input), "Input", names)};`,
			`export type ${name}Output = ${action.output === "null" ? "void" : qualifyShape(optionalizeNullableFields(action.output), "Output", names)};`,
		);
	}

	lines.push(
		"",
		`export interface AgentOsActions ${renderActionTree(buildTree(contract.publicActions))}`,
		"",
		"export type AgentOsEvents = {",
		...contract.events.map(
			(event) =>
				`\treadonly ${JSON.stringify(event.name)}: Type<${qualifyShape(optionalizeNullableFields(event.payload), "Output", names)}>;`,
		),
		"};",
		"",
		"export type AgentOsActorDefinition = BaseActorDefinition<",
		"\tundefined,",
		"\tundefined,",
		"\tundefined,",
		"\tundefined,",
		"\tAgentOsActorCreateInput,",
		"\tundefined,",
		"\tAgentOsEvents,",
		"\tRecord<never, never>,",
		"\tRecord<never, never>",
		">;",
		"export type AgentOsActorConnection = ActorConn<AgentOsActorDefinition> & AgentOsActions;",
		"export type AgentOsActorHandle = Omit<ActorHandle<AgentOsActorDefinition>, \"connect\"> &",
		"\tAgentOsActions & {",
		"\t\tconnect(params?: unknown, options?: ActorConnectOptions): AgentOsActorConnection;",
		"\t};",
		"export type AgentOsRegistry = Registry<{ agentOS: AgentOsActorDefinition }>;",
		"",
		"export type AgentOsCreateOptions = Omit<CreateOptions, \"input\"> & {",
		"\tinput?: AgentOsActorCreateInput;",
		"};",
		"export type AgentOsGetOrCreateOptions = Omit<",
		"\tGetOptions,",
		"\t\"createWithInput\"",
		"> & { createWithInput?: AgentOsActorCreateInput };",
		"export type AgentOsActorAccessor = Omit<",
		"\tActorAccessor<AgentOsActorDefinition>,",
		"\t\"create\" | \"getOrCreate\"",
		"> & {",
		"\tgetOrCreate(",
		"\t\tkey?: string | string[],",
		"\t\toptions?: AgentOsGetOrCreateOptions,",
		"\t): AgentOsActorHandle;",
		"\tcreate(",
		"\t\tkey?: string | string[],",
		"\t\toptions?: AgentOsCreateOptions,",
		"\t): Promise<AgentOsActorHandle>;",
		"};",
		"export type AgentOsClient = Omit<Client<AgentOsRegistry>, \"agentOS\" | \"getForId\"> & {",
		"\treadonly agentOS: AgentOsActorAccessor;",
		"\tgetForId(name: \"agentOS\", actorId: string, options?: GetWithIdOptions): AgentOsActorHandle;",
		"};",
		"",
		"/** Creates a vanilla RivetKit client with the generated agentOS contract. */",
		"export function createAgentOsClient(",
		"\tendpointOrConfig?: string | ClientConfigInput,",
		"): AgentOsClient {",
		"\treturn createClient<AgentOsRegistry>(endpointOrConfig) as unknown as AgentOsClient;",
		"}",
		"",
	);
	return lines.join("\n");
}

function formatTypeScript(source) {
	return execFileSync(
		"pnpm",
		["exec", "biome", "format", "--stdin-file-path", outputPath],
		{
			cwd: workspaceRoot,
			encoding: "utf8",
			input: source,
			maxBuffer: 16 * 1024 * 1024,
		},
	);
}

function canonicalSchema(contract) {
	const { publicActions: _publicActions, ...schema } = contract;
	return `${JSON.stringify(schema, null, 2)}\n`;
}

function generate() {
	const contract = readRustContract();
	return {
		schema: canonicalSchema(contract),
		typeScript: formatTypeScript(emitTypeScript(contract)),
	};
}

function checkFile(path, expected) {
	const actual = readFileSync(path, "utf8");
	assert.equal(actual, expected, `${path} is stale; run pnpm generate`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const command = process.argv[2];
	const generated = generate();
	if (command === "generate") {
		mkdirSync(dirname(schemaPath), { recursive: true });
		mkdirSync(dirname(outputPath), { recursive: true });
		writeFileSync(schemaPath, generated.schema);
		writeFileSync(outputPath, generated.typeScript);
	} else if (command === "check") {
		checkFile(schemaPath, generated.schema);
		checkFile(outputPath, generated.typeScript);
	} else {
		throw new Error("usage: node src/cli.mjs <generate|check>");
	}
}
