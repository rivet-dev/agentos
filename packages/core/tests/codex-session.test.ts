import { resolve } from "node:path";
import codex from "@rivet-dev/agent-os-codex-agent";
import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { REGISTRY_SOFTWARE } from "./helpers/registry-commands.js";

const MODULE_ACCESS_CWD = resolve(import.meta.dirname, "..");

describe("Codex agent availability", () => {
	const cleanups = new Set<() => Promise<void>>();

	afterEach(async () => {
		for (const stop of cleanups) {
			await stop();
		}
		cleanups.clear();
	});

	test("codex package provides commands without registering a runnable ACP agent", async () => {
		const vm = await AgentOs.create({
			moduleAccessCwd: MODULE_ACCESS_CWD,
			software: [codex, ...REGISTRY_SOFTWARE],
		});
		cleanups.add(async () => {
			await vm.dispose();
		});

		expect(vm.listAgents().some((agent) => agent.id === "codex")).toBe(false);
		await expect(vm.createSession("codex")).rejects.toThrow(
			"Unknown agent type: codex",
		);
	});
});
