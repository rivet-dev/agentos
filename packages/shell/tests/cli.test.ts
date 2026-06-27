import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vitest";

const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const cliPath = join(packageRoot, "dist", "main.js");

function runCli(args: string[], input?: string) {
	return spawnSync(process.execPath, [cliPath, ...args], {
		cwd: packageRoot,
		encoding: "utf8",
		input,
		timeout: 60_000,
	});
}

describe("agentos-shell cli", () => {
	test("--help prints Docker-style run flags without starting a VM", () => {
		const result = runCli(["--help"]);

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("agentos-shell");
		expect(result.stdout).toContain("-i, --interactive");
		expect(result.stdout).toContain("-t, --tty");
		expect(result.stdout).toContain("-e, --env <env>");
		expect(result.stdout).toContain("-v, --volume <spec>");
		expect(result.stderr).not.toContain("agent-os shell");
	});

	test("runs a VM-backed command with guest cwd and env", () => {
		const result = runCli([
			"--workdir",
			"/tmp",
			"--env",
			"SHELL_TEST_ENV=works",
			"--",
			"node",
			"-e",
			"console.log('SHELL_VM_COMMAND:' + process.cwd() + ':' + process.env.SHELL_TEST_ENV); process.exit(7);",
		]);

		expect(result.status).toBe(7);
		expect(result.stdout).toContain("SHELL_VM_COMMAND:/tmp:works");
	});

	test("mounts a host directory with Docker -v syntax", () => {
		const hostDir = mkdtempSync(join(tmpdir(), "agentos-shell-volume-"));
		writeFileSync(join(hostDir, "hello.txt"), "mounted\n");

		const result = runCli([
			"--volume",
			`${hostDir}:/mnt:ro`,
			"--",
			"cat",
			"/mnt/hello.txt",
		]);

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("mounted");
	});

	test("keeps stdin attached when -i is set", () => {
		const result = runCli(["--interactive", "--", "bash"], "echo hello\n");

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("hello");
	});

	test("runs a command through terminal mode when -t is set", () => {
		const result = runCli([
			"--workdir",
			"/tmp",
			"--tty",
			"--",
			"bash",
			"-c",
			"echo tty-mode",
		]);

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("tty-mode");
	});

	test("default terminal mode launches bash instead of the synthetic shell", () => {
		const result = runCli(["--interactive", "--tty"], "echo $0\nexit\n");

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("bash");
		expect(result.stdout).not.toContain("sh-0.4$");
	});

	test("reads env files", () => {
		const dir = mkdtempSync(join(tmpdir(), "agentos-shell-env-"));
		mkdirSync(join(dir, "nested"));
		const envFile = join(dir, "nested", "env.list");
		writeFileSync(envFile, "FROM_ENV_FILE=yes\n");

		const result = runCli([
			"--env-file",
			envFile,
			"--",
			"node",
			"-e",
			"console.log(process.env.FROM_ENV_FILE)",
		]);

		expect(result.status).toBe(0);
		expect(result.stdout).toContain("yes");
	});
});
