import { expect } from "vitest";
import type { AgentOs } from "../../src/index.js";

// A module snapshot is built with an empty environment, and a top-level read
// of `process.env` keeps the value it saw then. Every test process sets
// `LIVE_ENV`, so a module that exports `evaluatedIn` tells whether the process
// evaluated it or booted with it already evaluated.
export const LIVE_ENV = "MODULE_SNAPSHOT_TEST_LIVE";
export const EVALUATED_IN = `export const evaluatedIn = process.env.${LIVE_ENV} === undefined ? "snapshot" : "live";\n`;

export interface EvaluatedIn {
	evaluatedIn: "snapshot" | "live";
}

/** Run `node <entry>` in `vm` and parse the JSON it writes to stdout. */
export async function runNode<T>(
	vm: AgentOs,
	entry: string,
	args: string[] = [],
	options: { env?: Record<string, string>; cwd?: string } = {},
): Promise<T> {
	let stdout = "";
	let stderr = "";
	const { pid } = await vm.process.spawn("node", [entry, ...args], {
		...options,
		env: { [LIVE_ENV]: "1", ...options.env },
		onStdout: (chunk) => {
			stdout += Buffer.from(chunk).toString();
		},
		onStderr: (chunk) => {
			stderr += Buffer.from(chunk).toString();
		},
	});
	const exit = await vm.process.wait(pid);
	expect({ exitCode: exit.exitCode, stderr }).toEqual({
		exitCode: 0,
		stderr: "",
	});
	return JSON.parse(stdout) as T;
}

/**
 * The snapshot builds in the background after the first launch. Run `run`
 * until a process boots from a snapshot that `accept` takes.
 */
export async function waitForSnapshotBoot<T extends EvaluatedIn>(
	run: () => Promise<T>,
	accept: (report: T) => boolean = () => true,
	timeoutMs = 30_000,
): Promise<T> {
	const deadline = Date.now() + timeoutMs;
	for (;;) {
		const report = await run();
		if (report.evaluatedIn === "snapshot" && accept(report)) return report;
		if (Date.now() > deadline) {
			throw new Error("guest processes never booted from the module snapshot");
		}
		await new Promise((resolve) => setTimeout(resolve, 200));
	}
}
