import { afterEach, describe, expect, test } from "vitest";
import { AgentOs } from "../src/agent-os.js";

/**
 * The FS bootstrap (creating the POSIX dir tree) runs ONLY when there is no base
 * layer. When the bundled base IS present, the sidecar's embedded base layer
 * provides every POSIX dir and the host-side bootstrap emits nothing — so it
 * never reads `base-filesystem.json` and never clobbers base dir metadata.
 */
describe("kernel FS bootstrap (base vs no-base)", () => {
	let vm: AgentOs | undefined;

	afterEach(async () => {
		await vm?.dispose();
		vm = undefined;
	});

	test("with base layer: /tmp keeps base 1777 (bootstrap does not clobber)", async () => {
		vm = await AgentOs.create();
		const st = await vm.stat("/tmp");
		// sticky + 0777 == 1777, served straight from the base lower
		expect(st.mode & 0o7777).toBe(0o1777);
	}, 60_000);

	test("with base layer: agentOS gap-fill dirs still exist", async () => {
		vm = await AgentOs.create();
		for (const p of ["/etc/agentos", "/usr/local/bin"]) {
			expect(await vm.exists(p), `${p} must exist`).toBe(true);
		}
	}, 60_000);

	test("no base layer: bootstrap creates /tmp 1777 from the constant table", async () => {
		vm = await AgentOs.create({
			rootFilesystem: { disableDefaultBaseLayer: true },
		});
		const st = await vm.stat("/tmp");
		expect(st.mode & 0o7777).toBe(0o1777);
	}, 60_000);
});
