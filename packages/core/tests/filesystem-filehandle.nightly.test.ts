import { spawnSync } from "node:child_process";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

const payload = "0123456789abcdefghijklmnopqrstuvwxyz"
	.repeat(7282)
	.slice(0, 256 * 1024);
const cases = [
	{
		name: "sync writes",
		body: `
const fd = fs.openSync('/dev/stdout', 'w');
try {
  for (let offset = 0; offset < payload.length;) {
    const written = fs.writeSync(fd, payload, offset);
    if (written <= 0) throw new Error('write made no progress');
    offset += written;
  }
} finally { fs.closeSync(fd); }`,
	},
	{
		name: "FileHandle.write",
		body: `
const handle = await fs.promises.open('/dev/stdout', 'w');
try {
  for (let offset = 0; offset < payload.length;) {
    const { bytesWritten } = await handle.write(payload, offset);
    if (bytesWritten <= 0) throw new Error('write made no progress');
    offset += bytesWritten;
  }
} finally { await handle.close(); }`,
	},
	{
		name: "FileHandle.writeFile buffer",
		body: `
const handle = await fs.promises.open('/dev/stdout', 'w');
try { await handle.writeFile(payload); }
finally { await handle.close(); }`,
	},
	{
		name: "FileHandle.writeFile async iterable",
		body: `
const handle = await fs.promises.open('/dev/stdout', 'w');
try {
  await handle.writeFile((async function* () {
    yield payload.subarray(0, 100_003);
    yield payload.subarray(100_003);
  })());
} finally { await handle.close(); }`,
	},
];

test.each(
	cases,
)("$name delivers the complete payload through a shell pipe", async ({
	body,
}) => {
	const source = `
const fs = require('node:fs');
const payload = Buffer.from('0123456789abcdefghijklmnopqrstuvwxyz'.repeat(7282).slice(0, 256 * 1024));
(async () => {
${body}
})().catch(error => { console.error(error); process.exitCode = 1; });
`;
	// A shell pipe is a real pipe on Linux. Node's direct child stdout uses a
	// socket pair, which cannot be reopened through /dev/stdout there.
	const command = '"$1" -e "$2" | cat';
	const native = spawnSync(
		"/bin/sh",
		["-c", command, "sh", process.execPath, source],
		{
			encoding: "utf8",
			timeout: 10_000,
			maxBuffer: 1024 * 1024,
			env: { PATH: process.env.PATH },
		},
	);
	expect(native.error).toBeUndefined();
	expect(native.status, native.stderr).toBe(0);
	expect(native.stderr).toBe("");
	expect(native.stdout).toBe(payload);
	const vm = await AgentOs.create({ permissions: { network: "deny" } });
	try {
		const result = await vm.execArgv(
			"sh",
			["-c", command, "sh", "node", source],
			{
				timeoutMs: 10_000,
				output: { capture: "all" },
			},
		);
		expect(result.exitCode, result.stderr).toBe(native.status);
		expect(result.stderr).toBe("");
		expect(result.stdout.length).toBe(native.stdout.length);
		expect(result.stdout).toBe(native.stdout);
	} finally {
		await vm.dispose();
	}
}, 30_000);

test.each([
	{ name: "buffer", input: "data" },
	{
		name: "async iterable",
		input:
			"(async function* () { for (let offset = 0; offset < data.length; offset += 128 * 1024) yield data.subarray(offset, offset + 128 * 1024); })()",
	},
])("FileHandle.writeFile $name lets an abort timer run during a multi-chunk file write", async ({
	input,
}) => {
	const source = `
const fs = require('node:fs');
(async () => {
  const path = '/tmp/abort-write-' + process.pid + '.bin';
  const handle = await fs.promises.open(path, 'w');
  const controller = new AbortController();
  const data = Buffer.alloc(16 * 1024 * 1024, 65);
  const timer = setTimeout(() => controller.abort(), 1);
  let outcome = 'completed';
  try {
    await handle.writeFile(${input}, { signal: controller.signal });
  } catch (error) {
    outcome = error.name;
  } finally {
    clearTimeout(timer);
    const size = (await handle.stat()).size;
    await handle.close();
    await fs.promises.unlink(path);
    console.log(JSON.stringify({ outcome, size, total: data.length }));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });`;
	const native = spawnSync(process.execPath, ["-e", source], {
		encoding: "utf8",
		timeout: 10_000,
	});
	expect(native.error).toBeUndefined();
	expect(native.status, native.stderr).toBe(0);
	const nativeResult = JSON.parse(native.stdout.trim()) as {
		outcome: string;
		size: number;
		total: number;
	};
	expect(nativeResult.outcome).toBe("AbortError");
	expect(nativeResult.size).toBeLessThan(nativeResult.total);
	const vm = await AgentOs.create({ permissions: { network: "deny" } });
	try {
		const result = await vm.execArgv("node", ["-e", source], {
			timeoutMs: 10_000,
			output: { capture: "all" },
		});
		expect(result.exitCode, result.stderr).toBe(0);
		const guestResult = JSON.parse(result.stdout.trim()) as typeof nativeResult;
		expect(guestResult.outcome).toBe(nativeResult.outcome);
		expect(guestResult.size).toBeLessThan(guestResult.total);
	} finally {
		await vm.dispose();
	}
}, 30_000);
