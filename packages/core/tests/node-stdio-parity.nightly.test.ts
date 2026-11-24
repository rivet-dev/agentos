import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const cases = [
	[
		"standard descriptor identity",
		`console.log(JSON.stringify({ stdin: process.stdin.fd, stdout: process.stdout.fd, stderr: process.stderr.fd }));`,
	],
	[
		"explicit output encoding",
		`process.stdout.write("c3a9", "hex"); process.stderr.write("dGVzdA==", "base64");`,
	],
	[
		"closed stdout reports its error without emitting bytes",
		`
const fs = require('node:fs');
const output = process.stdout;
const events = [];
output.on('error', (error) => events.push('error:' + error.code));
output.on('close', () => events.push('close'));
fs.closeSync(1);
output.write('must-not-escape', (error) => {
  events.push('callback:' + (error?.code ?? 'success'));
  setImmediate(() => fs.writeSync(2, JSON.stringify(events)));
});
`,
	],
	[
		"reopened stdout writes only to its replacement file",
		`
const fs = require('node:fs');
const output = process.stdout;
const file = require('node:os').tmpdir() + '/stdio-reopen-' + process.pid;
fs.closeSync(1);
const fd = fs.openSync(file, 'w');
if (fd !== 1) throw new Error('expected reopened fd 1, got ' + fd);
output.write('stream:');
fs.writeSync(1, 'filesystem');
fs.closeSync(1);
fs.writeSync(2, fs.readFileSync(file));
fs.unlinkSync(file);
`,
	],
	[
		"cross-duplicated stdout follows stderr for both write surfaces",
		`
const result = require('node:child_process').spawnSync(process.execPath, ['-e', "const fs = require('node:fs'); process.stdout.write('out-stream:'); fs.writeSync(1, 'out-fs'); process.stderr.write('err-stream:'); fs.writeSync(2, 'err-fs');"], { stdio: ['ignore', 2, 1] });
if (result.error) throw result.error;
if (result.status !== 0) throw new Error('child failed: ' + result.status);
`,
	],
	[
		"inherited pipe preserves partial writes without public duplication",
		`
const cp = require('node:child_process');
const child = cp.spawn(process.execPath, ['-e', "(async () => { process.stdout.write(Buffer.alloc(96 * 1024, 65)); const fs = require('node:fs'), bytes = Buffer.alloc(96 * 1024, 66); for (let offset = 0; offset < bytes.length;) { try { offset += fs.writeSync(1, bytes, offset, bytes.length - offset); } catch (error) { if (error.code !== 'EAGAIN') throw error; await new Promise(setImmediate); } } process.stderr.write('child-stderr'); })().catch(error => { console.error(error); process.exitCode = 1; });"], { stdio: ['ignore', 'pipe', 'pipe'] });
const chunks = [], errors = [];
child.stdout.on('data', chunk => chunks.push(chunk));
child.stderr.on('data', chunk => errors.push(chunk));
child.on('error', error => { throw error; });
child.on('close', code => {
 const bytes = Buffer.concat(chunks);
 console.log(JSON.stringify({ code, length: bytes.length, first: bytes.subarray(0, 96 * 1024).every(byte => byte === 65), second: bytes.subarray(96 * 1024).every(byte => byte === 66), stderr: Buffer.concat(errors).toString() }));
});
`,
	],
] as const;

test.each(cases)(
	"Node stdio matches native: %s",
	async (_name, source) => {
		const native = spawnSync(process.execPath, ["-e", source], {
			encoding: "utf8",
			timeout: 5_000,
			env: { PATH: process.env.PATH },
		});
		expect(native.error).toBeUndefined();
		expect(native.status, native.stderr).toBe(0);
		const vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		try {
			const guest = await vm.process.execFile("node", ["-e", source], {
				timeoutMs: 5_000,
				output: { capture: "all" },
			});
			expect(guest.exitCode, guest.stderr).toBe(0);
			expect(guest.stdout).toBe(native.stdout);
			expect(guest.stderr).toBe(native.stderr);
		} finally {
			await vm.dispose();
		}
	},
	15_000,
);
