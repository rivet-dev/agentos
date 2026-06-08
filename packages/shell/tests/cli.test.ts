import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const cliPath = join(packageRoot, "dist", "main.js");

describe("agent-os-shell cli", () => {
	test("--help prints usage without starting a VM", () => {
		const result = spawnSync(process.execPath, [cliPath, "--help"], {
			cwd: packageRoot,
			encoding: "utf8",
		});

		expect(result.status).toBe(0);
		expect(result.stderr).toContain("Usage:");
		expect(result.stderr).toContain("agent-os-shell [--work-dir <path>]");
		expect(result.stderr).not.toContain("agent-os shell");
		expect(result.stdout).toBe("");
	});

	test("runs a VM-backed command and exits with the guest status", () => {
		const result = spawnSync(
			process.execPath,
			[
				cliPath,
				"--work-dir",
				"/tmp",
				"--",
				"node",
				"-e",
				"console.log('SHELL_VM_COMMAND:' + process.cwd()); process.exit(7);",
			],
			{
				cwd: packageRoot,
				encoding: "utf8",
				timeout: 60_000,
			},
		);

		expect(result.status).toBe(7);
		expect(result.stderr).toContain("agent-os shell");
		expect(result.stderr).toContain("cwd: /tmp");
		expect(result.stdout).toContain("SHELL_VM_COMMAND:/tmp");
	});
});
