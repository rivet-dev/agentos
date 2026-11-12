import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

test("descriptor filesystem methods are available as ESM named exports", async () => {
	const source = `
import { fchmod, fchmodSync, fchown, fchownSync, fdatasync, fdatasyncSync,
  fsync, fsyncSync, ftruncateSync, futimes, futimesSync } from 'node:fs';
console.log(JSON.stringify([fchmod, fchmodSync, fchown, fchownSync, fdatasync,
  fdatasyncSync, fsync, fsyncSync, ftruncateSync, futimes, futimesSync].map(fn => typeof fn)));
`;
	const native = spawnSync(
		process.execPath,
		["--input-type=module", "-e", source],
		{
			encoding: "utf8",
			timeout: 5_000,
		},
	);
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({ defaultSoftware: false });
	try {
		const guest = await vm.process.execFile(
			"node",
			["--input-type=module", "-e", source],
			{
				timeoutMs: 5_000,
				output: { capture: "all" },
			},
		);
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 15_000);
