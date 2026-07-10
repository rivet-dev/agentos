import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  createIntegrationKernel,
  type IntegrationKernelResult,
} from '@rivet-dev/agentos-vm-test-harness';

const repoRoot = resolve(import.meta.dirname, '../../../..');
const ledgerPath = resolve(repoRoot, 'crates/node-stdlib/suite/ledger.json');

describe('pinned Node public builtin ledger', () => {
  let context: IntegrationKernelResult | undefined;

  afterEach(async () => {
    await context?.dispose();
  });

  it('matches the exact expected pass set for the selected stdlib flavor', async () => {
    const ledger = JSON.parse(await readFile(ledgerPath, 'utf8'));
    const flavor = process.env.AGENTOS_JS_STDLIB === 'real' ? 'real' : 'legacy';
    const expected = ledger.cases
      .filter((entry: { legacy: string; real: string }) => entry[flavor] === 'pass')
      .map((entry: { id: string }) => entry.id);
    const expectedAccepted = ledger.cases
      .filter((entry: { legacy: string; real: string }) => entry[flavor] === 'fail-accepted')
      .map((entry: { id: string }) => entry.id);
    context = await createIntegrationKernel({ runtimes: ['wasmvm', 'node'] });
    const script = `
const ids = ${JSON.stringify(ledger.cases.map((entry: { id: string }) => entry.id))};
const passed = [];
const failed = [];
for (const id of ids) {
  try { require(id); passed.push(id); }
  catch (error) { failed.push({ id, code: error?.code, message: error?.message }); }
}
process.stdout.write(JSON.stringify({ passed, failed }));
`;
    await context.vfs.writeFile('/tmp/node-stdlib-public-ledger.cjs', script);
    const result = await context.kernel.exec('node /tmp/node-stdlib-public-ledger.cjs');
    expect(result.exitCode, result.stderr).toBe(0);
    const actual = JSON.parse(result.stdout);
    expect(actual.passed).toEqual(expected);
    expect(actual.failed.map((entry: { id: string }) => entry.id)).toEqual(expectedAccepted);
  }, 30_000);
});
