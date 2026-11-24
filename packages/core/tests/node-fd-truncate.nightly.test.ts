import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const source = `
const fs = require('node:fs');
const path = require('node:os').tmpdir() + '/truncate-errors-' + process.pid;
const rows = [];
try {
  for (const mode of ['r', 'r+']) {
    for (const length of [undefined, 0, 2, -1, 1.5, NaN, Infinity, null, '2', true, {}, Number.MIN_SAFE_INTEGER, Number.MIN_SAFE_INTEGER - 1, Number.MAX_SAFE_INTEGER + 1]) {
      fs.writeFileSync(path, 'abcd');
      const fd = fs.openSync(path, mode);
      try {
        try { fs.ftruncateSync(fd, length); rows.push(['ok', fs.fstatSync(fd).size]); }
        catch (error) { rows.push([error.code, error.syscall ?? null, fs.fstatSync(fd).size]); }
      } finally { fs.closeSync(fd); }
    }
  }
} finally { fs.unlinkSync(path); }
console.log(JSON.stringify(rows));
`;

test("Node ftruncate validates lengths and access modes like native Node", async () => {
	const native = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({ defaultSoftware: false });
	try {
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 5000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(JSON.parse(guest.stdout)).toEqual(JSON.parse(native.stdout));
	} finally {
		await vm.dispose();
	}
}, 15000);
