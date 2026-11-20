import coreutils from "@agentos-software/coreutils";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("exec and execArgv deliver the stdin option to the command exactly once", async () => {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		software: [coreutils],
	});
	try {
		// Direct spawn, guest shell, and argv paths each feed stdin separately.
		expect((await vm.exec("cat", { stdin: "x" })).stdout).toBe("x");
		expect((await vm.exec("cat | cat", { stdin: "x" })).stdout).toBe("x");
		expect((await vm.execArgv("cat", [], { stdin: "x" })).stdout).toBe("x");
	} finally {
		await vm.dispose();
	}
}, 120_000);
