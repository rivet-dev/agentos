import { execFileSync } from "node:child_process";
import {
	chmodSync,
	mkdirSync,
	mkdtempSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

/**
 * End-to-end Phase-1 proof: a package is projected into the single `/opt/agentos`
 * tree, mounted GUEST-NATIVE from its `package.tar` (no extraction), and its
 * `bin/` command resolves through a real `$PATH` walk + header dispatch.
 *
 * The package is hand-built (no npm) so the test is deterministic; it mirrors the
 * toolchain's output shape: a directory containing `package.tar`, whose root holds
 * `agentos-package.json` (name + version) + `bin/<cmd>`. The sidecar mounts the tar
 * directly and projects `/opt/agentos/pkgs/<name>/<version>` + `pkgs/<name>/current`
 * + `bin/<cmd>` leaf mounts.
 */
describe("agentos package projection (VM)", () => {
	let vm: AgentOs;
	let root: string;

	beforeAll(async () => {
		root = mkdtempSync(join(tmpdir(), "agentos-pkg-vm-"));
		// Build the package tree, then tar it into `<packDir>/package.tar` — the
		// projection input the sidecar mounts (directory projection is not supported).
		const pkgDir = join(root, "pkg");
		mkdirSync(join(pkgDir, "bin"), { recursive: true });
		writeFileSync(
			join(pkgDir, "agentos-package.json"),
			JSON.stringify({ name: "hello-cmd", version: "1.0.0" }, null, 2),
		);
		const binPath = join(pkgDir, "bin", "hello-cmd");
		writeFileSync(
			binPath,
			"#!/usr/bin/env node\nprocess.stdout.write('hello from agentos package\\n');\n",
		);
		// Commands must be executable (Linux x-bit) — a non-executable PATH match
		// is skipped (ENOENT) and a direct non-executable path is denied (EACCES).
		chmodSync(binPath, 0o755);
		const packDir = join(root, "packed");
		mkdirSync(packDir, { recursive: true });
		// `-C pkgDir .` roots the entries at the tar's top level (./agentos-package.json,
		// ./bin/hello-cmd), preserving the executable bit.
		execFileSync("tar", [
			"-cf",
			join(packDir, "package.tar"),
			"-C",
			pkgDir,
			".",
		]);

		vm = await AgentOs.create({
			defaultSoftware: false,
			software: [packDir],
		});
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
		if (root) rmSync(root, { recursive: true, force: true });
	});

	test("projects the package tree under /opt/agentos", async () => {
		expect(
			await vm.exists("/opt/agentos/pkgs/hello-cmd/1.0.0/bin/hello-cmd"),
		).toBe(true);
		expect(await vm.exists("/opt/agentos/bin/hello-cmd")).toBe(true);
	});

	// `exec` would route through `sh -c`; spawn the command directly so the test
	// isolates the package's $PATH resolution + header dispatch (no shell needed).
	async function runCommand(
		command: string,
	): Promise<{ code: number; out: string; err: string }> {
		let out = "";
		let err = "";
		const { pid } = vm.spawn(command, [], {
			onStdout: (data) => {
				out += new TextDecoder().decode(data);
			},
			onStderr: (data) => {
				err += new TextDecoder().decode(data);
			},
		});
		const code = await vm.waitProcess(pid);
		// Native-sidecar process_output events can arrive a few turns after the
		// exit notification; poll briefly until output lands (tiny stdout is the
		// first thing to get lost if snapshotted immediately).
		for (let i = 0; i < 20 && out === "" && err === ""; i++) {
			await new Promise((resolve) => setTimeout(resolve, 25));
		}
		return { code, out, err };
	}

	test("resolves the command via $PATH and dispatches by header", async () => {
		const { code, out, err } = await runCommand("hello-cmd");
		expect(err, `stderr: ${err}`).not.toContain("Error");
		expect(out).toContain("hello from agentos package");
		expect(code).toBe(0);
	});

	test("resolves the command by absolute path too", async () => {
		const { code, out, err } = await runCommand("/opt/agentos/bin/hello-cmd");
		expect(err, `stderr: ${err}`).not.toContain("Error");
		expect(out).toContain("hello from agentos package");
		expect(code).toBe(0);
	});

	test("runs repeatedly (no shebang corruption across executions)", async () => {
		// Re-executing a shebang command must not corrupt its on-disk source: the
		// JS import-cache write previously clobbered the read-only mount, stripping
		// the `#!` so the 2nd exec failed with ENOEXEC.
		for (let i = 0; i < 3; i++) {
			const { code, out, err } = await runCommand("hello-cmd");
			expect(err, `iteration ${i} stderr: ${err}`).not.toContain("Error");
			expect(out, `iteration ${i}`).toContain("hello from agentos package");
			expect(code).toBe(0);
		}
	});
});
