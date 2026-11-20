import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";

type Operation = {
	kind: number;
	from: string;
	to: string;
	data: string;
	size: number;
};

const seedCount = Number(process.env.AGENTOS_FS_MODEL_SEED_COUNT ?? 16);
if (!Number.isInteger(seedCount) || seedCount < 1 || seedCount > 512) {
	throw new Error(
		"AGENTOS_FS_MODEL_SEED_COUNT must be an integer from 1 to 512",
	);
}
const seeds = [
	...new Set([
		...Array.from({ length: seedCount }, (_, index) => index + 1),
		42,
		0x5eed,
	]),
];

function generatedOperations(seed: number): Operation[] {
	let state = seed;
	const random = () => {
		state ^= state << 13;
		state ^= state >>> 17;
		state ^= state << 5;
		return state >>> 0;
	};
	const paths = [
		"a",
		"b",
		"space name",
		"utf8-λ",
		"d/a",
		"d/b",
		"missing/a",
		"a/child",
	];
	return Array.from({ length: 128 }, () => ({
		kind: random() % 10,
		from: paths[random() % paths.length],
		to: paths[random() % paths.length],
		data: `seed-${seed}:${random().toString(16)}:λ\n`,
		size: random() % 40,
	}));
}

function probeSource(operations: Operation[]): string {
	return `
const fs = require('node:fs');
process.umask(0o022);
fs.mkdirSync('d');
const operations = ${JSON.stringify(operations)};
const capture = (operation) => {
  try { return { ok: operation() ?? null }; }
  catch (error) { return { error: error.code ?? error.name }; }
};
const snapshot = () => Object.fromEntries(['a', 'b', 'space name', 'utf8-λ', 'd/a', 'd/b'].map(path => [path, capture(() => {
  const stat = fs.lstatSync(path);
  if (stat.isSymbolicLink()) return { type: 'symlink', target: fs.readlinkSync(path) };
  return { type: 'file', mode: stat.mode & 0o777, size: stat.size, links: stat.nlink, data: fs.readFileSync(path).toString('hex') };
})]));
const results = operations.map(({ kind, from, to, data, size }) => {
  const result = capture(() => {
    switch (kind) {
      case 0: fs.writeFileSync(from, data); break;
      case 1: fs.appendFileSync(from, data); break;
      case 2: return fs.readFileSync(from).toString('hex');
      case 3: fs.renameSync(from, to); break;
      case 4: fs.unlinkSync(from); break;
      case 5: fs.linkSync(from, to); break;
      case 6: fs.symlinkSync(from, to); break;
      case 7: fs.truncateSync(from, size); break;
      case 8: fs.copyFileSync(from, to, fs.constants.COPYFILE_EXCL); break;
      case 9: fs.writeFileSync(from, data, { flag: 'wx' }); break;
    }
  });
  return { result, snapshot: snapshot() };
});
console.log(JSON.stringify(results));
`;
}

async function compareWithNative(vm: AgentOs, name: string, source: string) {
	const nativeCwd = mkdtempSync(join(tmpdir(), "agentos-fs-model-"));
	let native: unknown[];
	try {
		native = JSON.parse(
			execFileSync(process.execPath, ["-e", source], {
				cwd: nativeCwd,
				encoding: "utf8",
				timeout: 10_000,
				maxBuffer: 4 * 1024 * 1024,
				env: { PATH: process.env.PATH },
			}),
		);
	} finally {
		rmSync(nativeCwd, { recursive: true, force: true });
	}
	const cwd = `/tmp/fs-model-${name}`;
	await vm.mkdir(cwd, { recursive: true });
	const result = await vm.javascript.execute(source, {
		cwd,
		format: "commonjs",
		timeoutMs: 30_000,
		output: { capture: "all" },
	});
	expect(result.outcome, JSON.stringify(result)).toBe("succeeded");
	const actual: unknown[] = JSON.parse(result.stdout);
	return { actual, native };
}

describe("filesystem operation traces match Linux Node", () => {
	let vm: AgentOs;
	beforeAll(async () => {
		vm = await AgentOs.create({
			defaultSoftware: false,
			permissions: { network: "deny" },
		});
	}, 60_000);
	afterAll(async () => {
		await vm?.dispose();
	});

	test.each(seeds)("replays seed %i", async (seed) => {
		const operations = generatedOperations(seed);
		const { actual, native } = await compareWithNative(
			vm,
			String(seed),
			probeSource(operations),
		);
		expect(actual.length).toBe(operations.length);
		for (let step = 0; step < operations.length; step++) {
			expect(
				actual[step],
				`seed=${seed} step=${step} operation=${JSON.stringify(operations[step])}`,
			).toEqual(native[step]);
		}
	}, 60_000);

	test.each([
		{
			name: "dangling-append",
			body: `fs.symlinkSync('target', 'link'); fs.appendFileSync('link', 'data'); return [fs.readFileSync('target', 'utf8'), fs.readlinkSync('link')];`,
		},
		{
			name: "recreate-through-symlink",
			body: `fs.writeFileSync('target', 'old'); fs.symlinkSync('target', 'link'); fs.unlinkSync('target'); fs.appendFileSync('link', 'new'); return [fs.readFileSync('target', 'utf8'), fs.readFileSync('link', 'utf8')];`,
		},
		{
			name: "creation-umask",
			body: `process.umask(0o077); fs.writeFileSync('sync', 'data'); fs.appendFileSync('append', 'data'); await fs.promises.writeFile('async', 'data'); fs.mkdirSync('dir'); return ['sync', 'append', 'async', 'dir'].map(path => fs.statSync(path).mode & 0o777);`,
		},
		{
			name: "copy-permissions",
			body: `process.umask(0o077); fs.writeFileSync('source', 'data'); fs.chmodSync('source', 0o751); fs.copyFileSync('source', 'sync'); await fs.promises.copyFile('source', 'async'); fs.writeFileSync('existing', 'old'); fs.chmodSync('existing', 0o600); fs.copyFileSync('source', 'existing'); return ['sync', 'async', 'existing'].map(path => fs.statSync(path).mode & 0o777);`,
		},
		{
			name: "rename-same-inode-open-handles",
			body: `fs.writeFileSync('source', 'data'); fs.linkSync('source', 'alias'); const a = fs.openSync('source', 'r'); const b = fs.openSync('alias', 'r'); try { fs.renameSync('source', 'alias'); return [fs.readlinkSync('/proc/self/fd/' + a).split('/').pop(), fs.readlinkSync('/proc/self/fd/' + b).split('/').pop(), fs.statSync('source').nlink, fs.statSync('alias').nlink]; } finally { fs.closeSync(a); fs.closeSync(b); }`,
		},
		{
			name: "binary-append",
			body: `fs.writeFileSync('file', Buffer.from([0, 255, 128])); fs.appendFileSync('file', Buffer.from([254, 129, 0])); return [fs.readFileSync('file').toString('hex')];`,
		},
		{
			name: "async-binary-append",
			body: `fs.writeFileSync('file', Buffer.from([0, 255, 128])); await fs.promises.appendFile('file', Buffer.from([254, 129, 0])); await fs.promises.appendFile('file', 'ff00', 'hex'); return [fs.readFileSync('file').toString('hex')];`,
		},
		{
			name: "async-append-flags",
			body: `fs.writeFileSync('file', 'original'); let code; try { await fs.promises.appendFile('file', 'replacement', { flag: 'ax' }); } catch (error) { code = error.code; } const original = fs.readFileSync('file', 'utf8'); await fs.promises.appendFile('file', 'new', { flag: 'w' }); return [code, original, fs.readFileSync('file', 'utf8')];`,
		},

		{
			name: "encoded-write",
			body: `fs.writeFileSync('file', '00ff80fe', 'hex'); fs.appendFileSync('file', 'ff00', { encoding: 'hex' }); return [fs.readFileSync('file').toString('hex')];`,
		},
		{
			name: "async-exclusive-write",
			body: `await fs.promises.writeFile('file', 'original'); let code; try { await fs.promises.writeFile('file', 'replacement', { flag: 'wx' }); } catch (error) { code = error.code; } return [code, await fs.promises.readFile('file', 'utf8')];`,
		},
		{
			name: "async-exclusive-copy",
			body: `fs.writeFileSync('source', 'replacement'); fs.writeFileSync('dest', 'original'); let code; try { await fs.promises.copyFile('source', 'dest', fs.constants.COPYFILE_EXCL); } catch (error) { code = error.code; } return [code, fs.readFileSync('dest', 'utf8')];`,
		},
		{
			name: "callback-exclusive-copy",
			body: `fs.writeFileSync('source', 'replacement'); fs.writeFileSync('dest', 'original'); const code = await new Promise(resolve => fs.copyFile('source', 'dest', fs.constants.COPYFILE_EXCL, error => resolve(error?.code))); return [code, fs.readFileSync('dest', 'utf8')];`,
		},
	])("matches Node for $name", async ({ name, body }) => {
		const source = `const fs = require('node:fs'); (async () => { ${body} })().then(result => console.log(JSON.stringify(result)), error => { console.error(error); process.exitCode = 1; });`;
		const { actual, native } = await compareWithNative(vm, name, source);
		expect(actual).toEqual(native);
	});
});
