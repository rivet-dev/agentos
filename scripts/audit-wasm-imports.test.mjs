import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const script = join(dirname(fileURLToPath(import.meta.url)), 'audit-wasm-imports.mjs');
const u32 = (value) => {
  const bytes = [];
  do {
    let byte = value & 0x7f;
    value >>>= 7;
    if (value) byte |= 0x80;
    bytes.push(byte);
  } while (value);
  return bytes;
};
const string = (value) => [...u32(Buffer.byteLength(value)), ...Buffer.from(value)];
const section = (id, body) => [id, ...u32(body.length), ...body];
const wasm = (kind = 0) => Buffer.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  ...section(1, [1, 0x60, 2, 0x7f, 0x7e, 1, 0x7f]),
  ...section(2, [1, ...string('host_test'), ...string('call'), kind, 0]),
  // A large custom payload must not be disassembled just to inspect imports.
  ...section(0, [...string('padding'), ...Buffer.alloc(1024 * 1024)]),
]);

function runAudit(kind, declaredParams = ['i32', 'i64']) {
  const root = mkdtempSync(join(tmpdir(), 'agentos-wasm-audit-'));
  try {
    const commands = join(root, 'commands');
    mkdirSync(commands);
    writeFileSync(join(commands, 'fixture'), wasm(kind));
    const manifest = join(root, 'manifest.json');
    writeFileSync(manifest, JSON.stringify({
      schemaVersion: 2,
      abiVersion: 1,
      imports: [{ module: 'host_test', name: 'call', params: declaredParams, results: ['i32'], status: 'canonical' }],
    }));
    return spawnSync(process.execPath, [script, '--commands', commands, '--manifest', manifest], {
      encoding: 'utf8',
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test('reads function imports without disassembling code or data', () => {
  const result = runAudit(0);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /1 commands, 1 modules, 1 imports/);
});

test('rejects a mismatched function signature', () => {
  const result = runAudit(0, ['i32']);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /expected \(i32\) -> \(i32\), observed \(i32,i64\) -> \(i32\)/);
});

test('rejects non-function imports', () => {
  const result = runAudit(2);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /non-function import host_test\.call/);
});
