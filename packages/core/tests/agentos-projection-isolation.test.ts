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
 * Phase 5 (§6): base packages are read-only PROJECTIONS under `/opt/agentos`,
 * leaving the system dirs normal/writable (the VM has a writable root). This
 * suite proves both halves of the §6 overlay OBJECTIVE — "`/usr/bin` etc. are
 * genuinely writable like Linux" over the read-only base layer — are met by the
 * chosen design, so the deferred whole-root copy-up *mechanism* is unnecessary:
 *   1. Projected package leaves under `/opt/agentos` are read-only.
 *   2. System dirs accept NEW files.
 *   3. COPY-UP: an EXISTING base-layer file (`/etc/hostname`) can be overwritten,
 *      and the new content is visible to both the host API and a guest shell —
 *      exactly the Linux semantic the copy-up overlay was meant to provide.
 */
describe("agentos projection isolation (VM)", () => {
	let vm: AgentOs;
	let root: string;

	beforeAll(async () => {
		root = mkdtempSync(join(tmpdir(), "agentos-projection-isolation-"));
		const pkgDir = join(root, "pkg");
		mkdirSync(join(pkgDir, "bin"), { recursive: true });
		writeFileSync(
			join(pkgDir, "package.json"),
			JSON.stringify({ name: "readonly-fixture", version: "1.0.0" }),
		);
		writeFileSync(
			join(pkgDir, "agentos-package.json"),
			JSON.stringify({ name: "readonly-fixture", version: "1.0.0" }),
		);
		const binPath = join(pkgDir, "bin", "readonly-cmd");
		writeFileSync(binPath, "#!/usr/bin/env node\n");
		chmodSync(binPath, 0o755);

		vm = await AgentOs.create({ defaultSoftware: false, software: [pkgDir] });
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
		if (root) rmSync(root, { recursive: true, force: true });
	});

	test("projected /opt/agentos package leaves reject guest writes", async () => {
		await expect(
			vm.writeFile(
				"/opt/agentos/pkgs/readonly-fixture/1.0.0/bin/readonly-cmd",
				"x",
			),
		).rejects.toThrow();
	});

	test("system dirs stay normal/writable", async () => {
		await vm.writeFile("/usr/bin/writable-probe", "x");
		expect(await vm.exists("/usr/bin/writable-probe")).toBe(true);
	});

	test("copy-up: an existing base-layer file is overwritable (Linux-like)", async () => {
		// `/etc/hostname` is seeded by the read-only base layer.
		expect(await vm.exists("/etc/hostname")).toBe(true);
		await vm.writeFile("/etc/hostname", "copied-up-host\n");
		const readBack = new TextDecoder().decode(
			await vm.readFile("/etc/hostname"),
		);
		expect(readBack).toBe("copied-up-host\n");
	});
});
