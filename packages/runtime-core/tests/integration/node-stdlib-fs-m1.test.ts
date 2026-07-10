import { afterEach, describe, expect, it } from "vitest";
import {
	createIntegrationKernel,
	type IntegrationKernelResult,
} from "@rivet-dev/agentos-vm-test-harness";

describe("Node stdlib M1 filesystem parity", () => {
	let context: IntegrationKernelResult | undefined;

	afterEach(async () => {
		await context?.dispose();
	});

	it("reports ENOENT for sync and async readdir of a missing mapped path", async () => {
		context = await createIntegrationKernel({ runtimes: ["wasmvm", "node"] });
		const script = String.raw`
const fs = require('node:fs');
const missing = '/tmp/agentos-m1-missing-directory';
const observed = {};
try { fs.readdirSync(missing); } catch (error) {
  observed.sync = { code: error.code, syscall: error.syscall, path: error.path };
}
try { await fs.promises.readdir(missing); } catch (error) {
  observed.async = { code: error.code, syscall: error.syscall, path: error.path };
}
process.stdout.write(JSON.stringify(observed));
`;
		await context.vfs.writeFile(
			"/tmp/node-stdlib-fs-readdir-missing.mjs",
			script,
		);
		const result = await context.kernel.exec(
			"node /tmp/node-stdlib-fs-readdir-missing.mjs",
		);

		expect(result.exitCode, result.stderr).toBe(0);
		expect(JSON.parse(result.stdout)).toEqual({
			sync: {
				code: "ENOENT",
				syscall: "scandir",
				path: "/tmp/agentos-m1-missing-directory",
			},
			async: {
				code: "ENOENT",
				syscall: "scandir",
				path: "/tmp/agentos-m1-missing-directory",
			},
		});
	});

	it("uses the real Node CJS loader and fd-backed filesystem ABI", async () => {
		context = await createIntegrationKernel({ runtimes: ["wasmvm", "node"] });
		await context.vfs.writeFile(
			"/tmp/node-stdlib-m1-dependency.cjs",
			`
const fs = require('node:fs');
module.exports = {
  fs,
  read(path) { return fs.readFileSync(path, 'utf8'); },
};
`,
		);
		await context.vfs.writeFile(
			"/tmp/node-stdlib-m1-acceptance.cjs",
			`
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const dependency = require('./node-stdlib-m1-dependency.cjs');

(async () => {
  const root = fs.mkdtempSync('/tmp/agentos-node-m1-');
  const source = root + '/source.bin';
  const copy = root + '/copy.bin';
  const fd = fs.openSync(source, 'w+');
  const first = Buffer.from([0, 1, 2, 3, 254, 255]);
  fs.writeSync(fd, first, 0, first.length, 0);
  fs.writevSync(fd, [Buffer.from([4, 5]), Buffer.from([6, 7])], 6);
  const positional = Buffer.alloc(4);
  fs.readSync(fd, positional, 0, positional.length, 2);
  const vectors = [Buffer.alloc(3), Buffer.alloc(3)];
  fs.readvSync(fd, vectors, 0);
  fs.ftruncateSync(fd, 8);
  fs.fsyncSync(fd);
  fs.fdatasyncSync(fd);
  fs.futimesSync(fd, new Date(1_700_000_000_111), new Date(1_700_000_000_222));
  fs.fchmodSync(fd, 0o640);
  const ownership = fs.fstatSync(fd);
  fs.fchownSync(fd, ownership.uid, ownership.gid);
  const beforeClose = fs.fstatSync(fd);
  fs.closeSync(fd);

  fs.accessSync(source, fs.constants.R_OK | fs.constants.W_OK);
  fs.utimesSync(source, new Date(1_700_000_000_333), new Date(1_700_000_000_444));
  fs.copyFileSync(source, copy, fs.constants.COPYFILE_EXCL);
  const statfs = fs.statfsSync(root);
  const stat = fs.statSync(copy);
  const directory = fs.opendirSync(root);
  const names = [];
  for (let entry; (entry = directory.readSync()) !== null;) names.push(entry.name);
  directory.closeSync();

  const asyncFile = root + '/async.txt';
  const handle = await fsp.open(asyncFile, 'w+');
  await handle.write(Buffer.from('async-node-fs'), 0, 13, 0);
  await handle.sync();
  await handle.truncate(5);
  const asyncStat = await handle.stat();
  await handle.close();
  const asyncText = await fsp.readFile(asyncFile, 'utf8');
  const asyncNames = (await fsp.readdir(root)).sort();
  const closeCallback = await new Promise((resolve, reject) => {
    fs.open(copy, 'r', (openError, closeFd) => {
      if (openError) return reject(openError);
      fs.close(closeFd, (...args) => {
        const expected = [null];
        resolve({
          arrayPrototype: Object.getPrototypeOf(args) === Array.prototype,
          expectedArrayPrototype: Object.getPrototypeOf(expected) === Array.prototype,
          samePrototype: Object.getPrototypeOf(args) === Object.getPrototypeOf(expected),
          constructorEqual: args.constructor === expected.constructor,
        });
      });
    });
  });
  let invalidMode;
  try { fs.openSync(source, 'r', 'boom'); }
  catch (error) { invalidMode = { name: error.name, code: error.code, message: error.message }; }
  let invalidAsyncMode;
  try { fs.open(source, 'r', 'boom', () => {}); }
  catch (error) { invalidAsyncMode = { name: error.name, code: error.code, message: error.message }; }

  process.stdout.write(JSON.stringify({
    dependencyFsSame: dependency.fs === fs,
    dependencyRead: dependency.read(asyncFile),
    positional: [...positional],
    vectors: vectors.map((value) => [...value]),
    bytes: [...fs.readFileSync(copy)],
    beforeClose: { size: beforeClose.size, mode: beforeClose.mode & 0o777 },
    stat: {
      size: stat.size,
      inoPositive: stat.ino > 0,
      nlinkPositive: stat.nlink > 0,
      blksizePositive: stat.blksize > 0,
      blocksNonNegative: stat.blocks >= 0,
    },
    statfs: { bsizePositive: statfs.bsize > 0, blocksNonNegative: statfs.blocks >= 0 },
    realpath: fs.realpathSync(copy),
    names: names.sort(),
    asyncText,
    asyncSize: asyncStat.size,
    asyncNames,
    invalidMode,
    invalidAsyncMode,
    closeCallback,
  }));
})().catch((error) => { console.error(error); process.exitCode = 1; });
`,
		);

		const result = await context.kernel.exec(
			"node /tmp/node-stdlib-m1-acceptance.cjs",
		);
		expect(result.exitCode, result.stderr).toBe(0);
		const actual = JSON.parse(result.stdout);
		expect(actual).toMatchObject({
			dependencyFsSame: true,
			dependencyRead: "async",
			positional: [2, 3, 254, 255],
			vectors: [
				[0, 1, 2],
				[3, 254, 255],
			],
			bytes: [0, 1, 2, 3, 254, 255, 4, 5],
			beforeClose: { size: 8, mode: 0o640 },
			stat: {
				size: 8,
				inoPositive: true,
				nlinkPositive: true,
				blksizePositive: true,
				blocksNonNegative: true,
			},
			statfs: { bsizePositive: true, blocksNonNegative: true },
			asyncText: "async",
			asyncSize: 5,
			invalidMode: {
				name: "TypeError",
				code: "ERR_INVALID_ARG_VALUE",
			},
			invalidAsyncMode: {
				name: "TypeError",
				code: "ERR_INVALID_ARG_VALUE",
			},
			closeCallback: {
				arrayPrototype: true,
				expectedArrayPrototype: true,
				samePrototype: true,
				constructorEqual: true,
			},
		});
		expect(actual.realpath).toMatch(/^\/tmp\/agentos-node-m1-/);
		expect(actual.names).toEqual(["copy.bin", "source.bin"]);
		expect(actual.asyncNames).toEqual(["async.txt", "copy.bin", "source.bin"]);
	});

	it("keeps sync backing-store reads stable across allocation and detach stress", async () => {
		context = await createIntegrationKernel({ runtimes: ["wasmvm", "node"] });
		const fixture = Uint8Array.from(
			{ length: 64 * 1024 },
			(_, index) => index & 0xff,
		);
		await context.vfs.writeFile(
			"/tmp/node-stdlib-m1-backing-store.bin",
			fixture,
		);
		await context.vfs.writeFile(
			"/tmp/node-stdlib-m1-backing-store.cjs",
			`
const assert = require('node:assert');
const fs = require('node:fs');
const fd = fs.openSync('/tmp/node-stdlib-m1-backing-store.bin', 'r');
const target = Buffer.alloc(1024, 0xaa);
let checksum = 0;
for (let iteration = 0; iteration < 1000; iteration++) {
  const position = (iteration * 257) % (64 * 1024 - 257);
  const bytesRead = fs.readSync(fd, target, 13, 257, position);
  assert.strictEqual(bytesRead, 257);
  assert.strictEqual(target[12], 0xaa);
  assert.strictEqual(target[270], 0xaa);
  assert.strictEqual(target[13], position & 0xff);
  checksum = (checksum + target[13] + target[269]) >>> 0;
  // Create collection pressure between parked sync reads. None of these
  // allocations may move/detach the destination backing store mid-RPC.
  Array.from({ length: 32 }, () => new Uint8Array(4096));
}
fs.closeSync(fd);

const detached = new Uint8Array(16);
if (typeof detached.buffer.transfer === 'function') detached.buffer.transfer();
else structuredClone(detached.buffer, { transfer: [detached.buffer] });
let detachedError;
try { fs.readSync(fs.openSync('/tmp/node-stdlib-m1-backing-store.bin', 'r'), detached, 0, 1, 0); }
catch (error) { detachedError = { name: error.name, code: error.code }; }

let sharedError;
if (typeof SharedArrayBuffer === 'function') {
  try {
    const shared = new Uint8Array(new SharedArrayBuffer(16));
    fs.readSync(fs.openSync('/tmp/node-stdlib-m1-backing-store.bin', 'r'), shared, 0, 1, 0);
  } catch (error) { sharedError = { name: error.name, code: error.code }; }
}

let resizableError;
try {
  const resizable = new Uint8Array(new ArrayBuffer(16, { maxByteLength: 32 }));
  if (resizable.buffer.resizable) {
    fs.readSync(fs.openSync('/tmp/node-stdlib-m1-backing-store.bin', 'r'), resizable, 0, 1, 0);
  }
} catch (error) { resizableError = { name: error.name, code: error.code }; }

process.stdout.write(JSON.stringify({ checksum, detachedError, sharedError, resizableError }));
`,
		);

		const result = await context.kernel.exec(
			"node /tmp/node-stdlib-m1-backing-store.cjs",
		);
		expect(result.exitCode, result.stderr).toBe(0);
		const actual = JSON.parse(result.stdout);
		expect(actual.checksum).toBeGreaterThan(0);
		expect(actual.detachedError).toMatchObject({ name: "TypeError" });
		expect(actual.sharedError).toMatchObject({
			name: "TypeError",
			code: "ERR_INVALID_ARG_TYPE",
		});
		expect(actual.resizableError).toMatchObject({
			name: "TypeError",
			code: "ERR_INVALID_ARG_TYPE",
		});
	}, 30_000);
});
