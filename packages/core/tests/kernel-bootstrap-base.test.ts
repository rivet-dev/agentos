import { afterEach, describe, expect, test, vi } from "vitest";
import { AgentOs } from "../src/agent-os.js";
import { SidecarProcess } from "../src/sidecar-process.js";

/**
 * The sidecar completes the POSIX directory tree before publishing the VM.
 * Existing base and caller-provided metadata remain authoritative.
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

	test("no base layer: sidecar creates /tmp with mode 1777", async () => {
		vm = await AgentOs.create({
			rootFilesystem: { disableDefaultBaseLayer: true },
		});
		const st = await vm.stat("/tmp");
		expect(st.mode & 0o7777).toBe(0o1777);
	}, 60_000);
	test.each([
		undefined,
		true,
		false,
	] as const)("read-only root completes directories and software with defaultSoftware=%s", async (defaultSoftware) => {
		vm = await AgentOs.create({
			...(defaultSoftware === undefined ? {} : { defaultSoftware }),
			rootFilesystem: { mode: "read-only" },
			permissions: {
				fs: "allow",
				childProcess: "allow",
				process: "allow",
				env: "allow",
				network: "deny",
				hostFunction: "deny",
			},
		});
		for (const path of ["/etc/agentos", "/usr/local/bin", "/boot"]) {
			expect(await vm.filesystem.exists(path), path).toBe(true);
		}
		expect((await vm.filesystem.stat("/tmp")).mode & 0o7777).toBe(0o1777);
		await expect(
			vm.writeFile("/tmp/readonly-command-projection", "blocked"),
		).rejects.toThrow("EROFS");
		if (defaultSoftware !== false) {
			for (const path of ["/bin/[", "/bin/ls", "/opt/agentos/bin/ls"]) {
				expect(await vm.filesystem.exists(path), path).toBe(true);
			}
			const bytes = await vm.readFile("/opt/agentos/bin/ls");
			expect(Array.from(bytes.subarray(0, 4))).toEqual([0, 97, 115, 109]);
			expect((await vm.execArgv("ls", ["/"])).exitCode).toBe(0);
			expect((await vm.execArgv("/bin/ls", ["/"])).exitCode).toBe(0);
		}
		const result = await vm.javascript.execute(`
				import fs from "node:fs";
				let code;
				try { fs.writeFileSync("/tmp/readonly-command-guest", "blocked"); }
				catch (error) { code = error.code; }
				if (code !== "EROFS") throw new Error("expected EROFS, received " + code);
			`);
		expect(result.outcome).toBe("succeeded");
	}, 60_000);

	test("creation avoids exporting the root while explicit export still works", async () => {
		const snapshots = vi.spyOn(
			SidecarProcess.prototype,
			"snapshotRootFilesystem",
		);
		try {
			vm = await AgentOs.create({ defaultSoftware: false });
			expect(snapshots).not.toHaveBeenCalled();
			const snapshot = await vm.filesystem.export({ maxBytes: 1024 * 1024 });
			expect(snapshots).toHaveBeenCalledTimes(1);
			expect(
				snapshot.source.filesystem.entries.some(
					(entry) => entry.path === "/etc/agentos",
				),
			).toBe(true);
		} finally {
			snapshots.mockRestore();
		}
	}, 60_000);
});
