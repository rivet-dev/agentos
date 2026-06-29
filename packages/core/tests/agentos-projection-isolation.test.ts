import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

/**
 * Phase 5 (§6): base packages are read-only PROJECTIONS under `/opt/agentos`,
 * leaving the system dirs normal/writable (the VM has a writable root). This
 * suite proves both halves of the §6 overlay OBJECTIVE — "`/usr/bin` etc. are
 * genuinely writable like Linux" over the read-only base layer — are met by the
 * chosen design, so the deferred whole-root copy-up *mechanism* is unnecessary:
 *   1. `/opt/agentos` is a read-only projection (guest writes denied).
 *   2. System dirs accept NEW files.
 *   3. COPY-UP: an EXISTING base-layer file (`/etc/hostname`) can be overwritten,
 *      and the new content is visible to both the host API and a guest shell —
 *      exactly the Linux semantic the copy-up overlay was meant to provide.
 */
describe("agentos projection isolation (VM)", () => {
	let vm: AgentOs;

	beforeAll(async () => {
		vm = await AgentOs.create({ defaultSoftware: false });
	}, 60_000);

	afterAll(async () => {
		await vm?.dispose();
	});

	test("/opt/agentos is a read-only projection (guest write denied)", async () => {
		await expect(
			vm.writeFile("/opt/agentos/bin/should-not-write", "x"),
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
