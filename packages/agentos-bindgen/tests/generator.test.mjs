import assert from "node:assert/strict";
import test from "node:test";

import {
	emitTypeScript,
	validateContract,
	validateDottedNames,
} from "../src/cli.mjs";

test("rejects action/group path collisions", () => {
	assert.throws(() => validateDottedNames(["network.fetch", "network.fetch.start"]));
});

test("reconstructs dotted actions as a nested type tree", () => {
	const output = emitTypeScript({
		apiVersion: "agentos-sdk.dev/v1alpha1",
		contractMajor: 1,
		contractHash: `sha256:${"a".repeat(64)}`,
		capabilities: ["network.pull-streaming"],
		types: [
			{ name: "JsonValue", declaration: "type JsonValue = string | null;" },
			{ name: "Request", declaration: "type Request = { value: bigint, };" },
		],
		publicActions: [
			{ name: "config.get", input: "null", output: "Request" },
			{ name: "network.fetchStream.start", input: "Request", output: "null" },
		],
		events: [{ name: "vm.booted", payload: "Request" }],
	});
	assert.match(output, /readonly fetchStream: \{/);
	assert.match(output, /readonly start: \(input: NetworkFetchStreamStartInput\)/);
	assert.match(output, /export type ConfigGetOutput = Output\.Request/);
	assert.match(output, /Input \{[\s\S]*value: number \| bigint/);
	assert.match(output, /export type NetworkFetchStreamStartOutput = void/);
	assert.match(output, /export type AgentOsActorConnection = ActorConn/);
	assert.match(output, /connect\(params\?: unknown, options\?: ActorConnectOptions\)/);
	assert.doesNotMatch(output, /session\./);
});

test("retains reserved actions in the IR but omits them from the client", () => {
	const contract = validateContract({
		schemaVersion: 1,
		actorName: "agentOS",
		apiVersion: "agentos-sdk.dev/v1alpha1",
		contractMajor: 1,
		contractHash: `sha256:${"a".repeat(64)}`,
		capabilities: [],
		actions: [
			{ name: "vm.status", public: true, input: "null", output: "null" },
			{
				name: "__agentos.cron.invoke",
				public: false,
				input: "null",
				output: "null",
			},
		],
		events: [],
		types: [],
	});
	const output = emitTypeScript(contract);
	assert.match(output, /readonly vm:/);
	assert.doesNotMatch(output, /__agentos/);
});
