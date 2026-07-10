import { afterEach, describe, expect, it } from "vitest";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
	C_BUILD_DIR,
	COMMANDS_DIR,
	createIntegrationKernel,
	createWasmVmRuntime,
	describeIf,
} from "@agentos/test-harness";
import type { IntegrationKernelResult } from "@agentos/test-harness";

const hasCommands = ["ps", "pgrep", "pkill"].every((command) =>
	existsSync(resolve(C_BUILD_DIR, command)),
);

describe("captured real-Linux process-consumer fixture", () => {
	it("records procps-ng and psmisc parsing the same live process", () => {
		const fixture = JSON.parse(
			readFileSync(
				new URL("./fixtures/linux-procps-ng-4.0.6.json", import.meta.url),
				"utf8",
			),
		);

		expect(fixture.procps_version).toMatch(/^ps from procps-ng /);
		expect(fixture.psmisc_version).toMatch(/^pstree \(PSmisc\) /);
		expect(fixture.target_found).toBe(true);
		expect(fixture.ps_header).toMatch(/PID\s+PPID\s+STAT\s+COMMAND/);
		expect(fixture.pstree_output).toBe(`aos-procfx(${fixture.target_pid})`);
		expect(fixture.prtstat_output).toContain("Process: aos-procfx");
	});
});

describeIf(hasCommands, "upstream procps-ng against live VM processes", () => {
	let ctx: IntegrationKernelResult;

	afterEach(async () => {
		await ctx?.dispose().catch(() => {});
	});

	it("ps and pgrep identify a live child, then pkill terminates it", async () => {
		ctx = await createIntegrationKernel({ runtimes: [] });
		await ctx.kernel.mount(
			createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
		);

		const sleeper = ctx.kernel.spawn("sleep", ["60"]);
		await new Promise((resolveReady) => setTimeout(resolveReady, 100));

		const pgrep = await ctx.kernel.exec("pgrep -x sleep");
		expect(pgrep.exitCode, pgrep.stderr).toBe(0);
		const guestPid = Number.parseInt(pgrep.stdout.trim(), 10);
		expect(guestPid).toBeGreaterThan(0);

		const ps = await ctx.kernel.exec("ps -eo pid,ppid,stat,comm,args");
		expect(ps.exitCode, ps.stderr).toBe(0);
		expect(ps.stdout).toMatch(/\bPID\b.*\bPPID\b.*\bSTAT\b.*\bCOMMAND\b/);
		expect(ps.stdout).toMatch(new RegExp(`^\\s*${guestPid}\\s+`, "m"));
		expect(ps.stdout).toContain("sleep 60");

		const pkill = await ctx.kernel.exec("pkill -TERM -x sleep");
		expect(pkill.exitCode, pkill.stderr).toBe(0);
		await expect(sleeper.wait()).resolves.not.toBe(0);

		const gone = await ctx.kernel.exec("pgrep -x sleep");
		expect(gone.exitCode).toBe(1);
	}, 60_000);
});
