import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { OPT_AGENTOS_BIN } from "../src/index.js";

// Agents are resolved DYNAMICALLY from the configured `/opt/agentos` package
// manifests (keyed by manifest `name`) plus the `@agentos-software/*` dependency
// agents linked lazily on first `createSession`. There is no hardcoded registry.
describe("listAgents()", () => {
	let vm: AgentOs;

	beforeEach(async () => {
		vm = await AgentOs.create();
	});

	afterEach(async () => {
		await vm.dispose();
	});

	test("lists the shipped dependency agents", () => {
		const agents = vm.listAgents();
		const ids = agents.map((a) => a.id);
		expect(ids).toContain("pi");
		expect(ids).toContain("pi-cli");
		expect(ids).toContain("opencode");
		expect(ids).toContain("claude");
	});

	test("each entry exposes a pre-resolved /opt/agentos adapter entrypoint", () => {
		const agents = vm.listAgents();
		for (const id of ["pi", "pi-cli", "opencode", "claude"]) {
			const agent = agents.find((entry) => entry.id === id);
			expect(agent).toBeDefined();
			// Every agent is an `/opt/agentos` package: the entry carries a
			// pre-resolved guest command path and no legacy npm adapter metadata.
			expect(agent?.adapterEntrypoint?.startsWith(`${OPT_AGENTOS_BIN}/`)).toBe(
				true,
			);
			expect(agent?.acpAdapter).toBeUndefined();
			expect(agent?.agentPackage).toBeUndefined();
			expect(typeof agent?.installed).toBe("boolean");
		}
	});

	test("every agent package is materialized at boot, so installed is true", () => {
		const agents = vm.listAgents();
		for (const agent of agents) {
			expect(agent.installed).toBe(true);
		}
	});
});
