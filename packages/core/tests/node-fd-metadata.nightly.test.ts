import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const cases = [
	[
		"chmod unlinked descriptor",
		"fs.fchmodSync(fd, 0o640); return fs.fstatSync(fd).mode & 0o777;",
	],
	[
		"chown unlinked descriptor",
		"const stat = fs.fstatSync(fd); fs.fchownSync(fd, stat.uid, stat.gid); fs.fchownSync(fd, -1, -1); return true;",
	],
	[
		"chmod original descriptor after path replacement",
		"fs.writeFileSync(path, 'replacement', { mode: 0o600 }); fs.fchmodSync(fd, 0o640); return [fs.fstatSync(fd).mode & 0o777, fs.statSync(path).mode & 0o777];",
	],
	[
		"closed descriptor errors",
		"fs.closeSync(fd); return ['fchmodSync', 'fchownSync'].map(method => { try { fs[method](fd, 0, 0); return 'success'; } catch (error) { return [error.code, error.syscall]; } });",
	],
] as const;

test.each(cases)(
	"Node descriptor metadata matches native: %s",
	async (_name, operation) => {
		const source = `const fs = require('node:fs'); const path = require('node:os').tmpdir() + '/fd-metadata-' + process.pid;
const fd = fs.openSync(path, 'w+', 0o600); fs.unlinkSync(path);
try { console.log(JSON.stringify((() => { ${operation} })())); }
finally { try { fs.closeSync(fd); } catch (error) { if (error.code !== 'EBADF') throw error; } if (fs.existsSync(path)) fs.unlinkSync(path); }`;
		const native = spawnSync(process.execPath, ["-e", source], {
			encoding: "utf8",
			timeout: 5000,
		});
		expect(native.error).toBeUndefined();
		expect(native.status, native.stderr).toBe(0);
		const vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
		try {
			const guest = await vm.process.execFile("node", ["-e", source], {
				timeoutMs: 5000,
				output: { capture: "all" },
			});
			expect(guest.exitCode, guest.stderr).toBe(0);
			expect(guest.stdout).toBe(native.stdout);
		} finally {
			await vm.dispose();
		}
	},
	15000,
);

test("FileHandle metadata retains the opened inode after unlink", async () => {
	const source = `(async () => {
 const fs = require('node:fs'); const path = require('node:os').tmpdir() + '/handle-metadata-' + process.pid;
 const handle = await fs.promises.open(path, 'w+', 0o600);
 await fs.promises.unlink(path);
 try { await handle.chmod(0o640); const before = await handle.stat(); await handle.chown(before.uid, before.gid); console.log(JSON.stringify((await handle.stat()).mode & 0o777)); }
 finally { await handle.close(); }
 })().catch(error => { console.error(error); process.exitCode = 1; });`;
	const native = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: { network: "deny" },
	});
	try {
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 5000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 15000);

test("Node fstat follows an unlinked duplicated inode after truncate", async () => {
	const source = `const fs = require('node:fs');
const path = require('node:os').tmpdir() + '/fdstat-' + process.pid;
const first = fs.openSync(path, 'w+', 0o600);
fs.writeSync(first, Buffer.from('abcdef'));
fs.ftruncateSync(first, 5);
const firstSize = fs.fstatSync(first).size;
fs.writeSync(first, Buffer.from('f'), 0, 1, 5);
const alias = fs.openSync('/dev/fd/' + first, 'r+');
fs.ftruncateSync(alias, 5);
const linkedSize = fs.fstatSync(alias).size;
fs.writeSync(alias, Buffer.from('f'), 0, 1, 5);
fs.closeSync(first);
fs.fchmodSync(alias, 0);
fs.unlinkSync(path);
fs.writeSync(alias, Buffer.from('xy'), 0, 2, 0);
const before = fs.fstatSync(alias);
fs.ftruncateSync(alias, 4);
const after = fs.fstatSync(alias);
const content = Buffer.alloc(10);
const count = fs.readSync(alias, content, 0, 10, 0);
console.log(JSON.stringify({ firstSize, linkedSize, before: before.size, after: after.size, count, bytes: [...content.subarray(0, count)], mode: after.mode & 0o777 }));
fs.closeSync(alias);`;
	const native = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: { network: "deny" },
	});
	try {
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 5000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 15000);

test.each([
	"fsyncSync",
	"fdatasyncSync",
])("Node %s validates a closed descriptor", async (method) => {
	const source = `const fs = require('node:fs');
const path = require('node:os').tmpdir() + '/fd-sync-' + process.pid;
const fd = fs.openSync(path, 'w+', 0o600);
fs.closeSync(fd);
fs.unlinkSync(path);
try { fs['${method}'](fd); console.log('success'); }
catch (error) { console.log(JSON.stringify([error.code, error.syscall])); }`;
	const native = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 5000,
	});
	expect(native.status, native.stderr).toBe(0);
	const vm = await AgentOs.create({
		defaultSoftware: false,
		permissions: { network: "deny" },
	});
	try {
		const guest = await vm.process.execFile("node", ["-e", source], {
			timeoutMs: 5000,
			output: { capture: "all" },
		});
		expect(guest.exitCode, guest.stderr).toBe(0);
		expect(guest.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 15000);
