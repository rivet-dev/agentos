import { afterEach, expect, it } from "vitest";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import {
	C_BUILD_DIR,
	COMMANDS_DIR,
	createIntegrationKernel,
	createWasmVmRuntime,
	describeIf,
} from "@agentos/test-harness";
import type { IntegrationKernelResult } from "@agentos/test-harness";

const hasCommands = ["pstree", "killall", "prtstat"].every((command) =>
	existsSync(resolve(C_BUILD_DIR, command)),
);

describeIf(hasCommands, "upstream psmisc against live VM processes", () => {
	let ctx: IntegrationKernelResult;

	afterEach(async () => {
		await ctx?.dispose().catch(() => {});
	});

	it("pstree and prtstat inspect a child, then killall terminates it", async () => {
		ctx = await createIntegrationKernel({ runtimes: [] });
		await ctx.kernel.mount(
			createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
		);

		const sleeper = ctx.kernel.spawn("sleep", ["60"]);
		await new Promise((resolveReady) => setTimeout(resolveReady, 100));

		const tree = await ctx.kernel.exec("pstree -p");
		expect(tree.exitCode, tree.stderr).toBe(0);
		const guestPid = Number.parseInt(
			/sleep\((\d+)\)/.exec(tree.stdout)?.[1] ?? "0",
			10,
		);
		expect(guestPid).toBeGreaterThan(0);

		const stat = await ctx.kernel.exec(`prtstat ${guestPid}`);
		expect(stat.exitCode, stat.stderr).toBe(0);
		expect(stat.stdout).toContain(`Process: sleep`);
		expect(stat.stdout).toContain(`State:`);

		const killed = await ctx.kernel.exec("killall sleep");
		expect(killed.exitCode, killed.stderr).toBe(0);
		await expect(sleeper.wait()).resolves.not.toBe(0);
	}, 60_000);
});
