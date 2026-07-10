import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  COMMANDS_DIR,
  createIntegrationKernel,
  type IntegrationKernelResult,
} from '@rivet-dev/agentos-vm-test-harness';

const repoRoot = resolve(import.meta.dirname, '../../../..');
const opensslCommandDir = resolve(repoRoot, 'toolchain/c/build');
const fixtureDir = resolve(repoRoot, 'packages/runtime-benchmarks/fixtures');

describe('shared Node OpenSSL wasm artifact', () => {
  let context: IntegrationKernelResult | undefined;

  afterEach(async () => {
    await context?.dispose();
  });

  it('completes an in-VM TLS handshake using tls-loopback-cert.pem', async () => {
    context = await createIntegrationKernel({
      runtimes: ['wasmvm'],
      commandDirs: [COMMANDS_DIR, opensslCommandDir],
    });
    await context.vfs.writeFile(
      '/tmp/tls-loopback-cert.pem',
      await readFile(resolve(fixtureDir, 'tls-loopback-cert.pem')),
    );
    await context.vfs.writeFile(
      '/tmp/tls-loopback-key.pem',
      await readFile(resolve(fixtureDir, 'tls-loopback-key.pem')),
    );

    const result = await context.kernel.exec(
      'openssl_handshake_smoke /tmp/tls-loopback-cert.pem /tmp/tls-loopback-key.pem',
    );
    expect(result.exitCode, result.stderr).toBe(0);
    expect(result.stdout).toContain('OpenSSL 3.5.5');
    expect(result.stdout).toContain('protocol=TLSv1.3');
    expect(result.stdout).toContain('encrypted-bytes=21');
  });
});
