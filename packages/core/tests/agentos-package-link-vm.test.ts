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
 * Phase 4 — runtime dynamic linking. `linkSoftware()` adds a package to the
 * already-running VM; its `bin/` command must resolve live (the `/opt/agentos`
 * mount is host-backed, so writing into the staging dir is reflected with no
 * reboot).
 */
describe("agentos linkSoftware (VM)", () => {
	let vm: AgentOs;
	let root: string;
	let pkgDir: string;

	beforeAll(async () => {
		root = mkdtempSync(join(tmpdir(), "agentos-link-"));
		pkgDir = join(root, "pkg");
		mkdirSync(join(pkgDir, "bin"), { recursive: true });
		writeFileSync(
			join(pkgDir, "package.json"),
			JSON.stringify({ name: "linked-tool", version: "1.0.0" }),
		);
		writeFileSync(
			join(pkgDir, "agentos-package.json"),
			JSON.stringify({ name: "linked-tool" }),
		);
		const binPath = join(pkgDir, "bin", "linked-cmd");
		writeFileSync(
			binPath,
			"#!/usr/bin/env node\nprocess.stdout.write('linked-cmd ran\\n');\n",
		);
		chmodSync(binPath, 0o755);
		// Boot with no packages — the /opt/agentos mount is created empty so we can
		// link into it at runtime.
		vm = await AgentOs.create({ defaultSoftware: false });
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
		if (root) rmSync(root, { recursive: true, force: true });
	});

	test("command does not exist before linking", async () => {
		expect(await vm.exists("/opt/agentos/bin/linked-cmd")).toBe(false);
	});

	test("linkSoftware makes the command resolve live via $PATH", async () => {
		await vm.linkSoftware(pkgDir);
		expect(await vm.exists("/opt/agentos/bin/linked-cmd")).toBe(true);

		let out = "";
		const { pid } = vm.spawn("linked-cmd", [], {
			onStdout: (d) => {
				out += new TextDecoder().decode(d);
			},
		});
		const code = await vm.waitProcess(pid);
		for (let i = 0; i < 20 && out === ""; i++) {
			await new Promise((r) => setTimeout(r, 25));
		}
		expect(code).toBe(0);
		expect(out).toContain("linked-cmd ran");
	});

	test("re-linking the same package is an idempotent no-op", async () => {
		// Projecting an already-projected `<name>/<version>` is idempotent (two
		// meta-packages can pull in the same sub-package). The sidecar returns the
		// package's commands without re-staging or erroring.
		await expect(
			vm.linkSoftware(pkgDir),
		).resolves.toBeUndefined();
		expect(await vm.exists("/opt/agentos/bin/linked-cmd")).toBe(true);
	});

	test("a different package re-providing a command is rejected", async () => {
		// The sidecar owns the projection now, so the duplicate-command rejection
		// surfaces from it ("already provided by another package"): a DIFFERENT
		// package whose `bin/` collides with an already-linked command name.
		const otherDir = join(root, "other");
		mkdirSync(join(otherDir, "bin"), { recursive: true });
		writeFileSync(
			join(otherDir, "package.json"),
			JSON.stringify({ name: "other-tool", version: "1.0.0" }),
		);
		writeFileSync(
			join(otherDir, "agentos-package.json"),
			JSON.stringify({ name: "other-tool" }),
		);
		const otherBin = join(otherDir, "bin", "linked-cmd");
		writeFileSync(
			otherBin,
			"#!/usr/bin/env node\nprocess.stdout.write('other ran\\n');\n",
		);
		chmodSync(otherBin, 0o755);
		await expect(vm.linkSoftware(otherDir)).rejects.toThrow(
			/already provided by another package/,
		);
	});
});
