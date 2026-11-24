import { afterAll, describe, expect, it, beforeAll } from 'vitest';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { once } from 'node:events';
import { createKernel, createInMemoryFileSystem, createWasmVmRuntime, C_BUILD_DIR, COMMANDS_DIR, allowAll } from '@rivet-dev/agentos-test-harness';

// Explicit expensive suite: missing protocol artifacts are failures, never skips.
describe('real upstream curl protocol backends', () => {
  let directory: string;
  let server: ChildProcess;
  let port: number;
  const payload = Buffer.from(Array.from({ length: 128 * 1024 }, (_, i) => i % 251));
  beforeAll(async () => {
    directory = mkdtempSync(join(tmpdir(), 'curl-protocol-'));
    execFileSync('openssl', ['req', '-new', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1', '-keyout', join(directory, 'key.pem'), '-out', join(directory, 'cert.pem')], { timeout: 10000, stdio: 'pipe' });
    server = spawn(process.execPath, [resolve(import.meta.dirname, 'fixtures/http2-server.mjs'), join(directory, 'key.pem'), join(directory, 'cert.pem')], { stdio: ['ignore', 'pipe', 'pipe'] });
    server.stderr!.on('data', data => process.stderr.write(data));
    port = await new Promise<number>((resolvePort, reject) => {
      const timer = setTimeout(() => reject(new Error('HTTP/2 fixture startup exceeded 5 seconds')), 5000);
      let output = '';
      server.on('error', reject);
      server.stdout!.on('data', data => {
        output += data.toString();
        if (output.length > 4096) { clearTimeout(timer); reject(new Error('fixture readiness output exceeded 4096 bytes')); return; }
        if (output.includes('\n')) {
          clearTimeout(timer);
          try { resolvePort(JSON.parse(output.split('\n')[0]).port); } catch (error) { reject(error); }
        }
      });
      server.once('exit', code => { clearTimeout(timer); reject(new Error(`fixture exited before readiness: ${code}`)); });
    });
  }, 15000);
  afterAll(async () => {
    if (server && server.exitCode === null) {
      const exited = once(server, 'exit');
      server.kill('SIGTERM');
      const timer = setTimeout(() => server.kill('SIGKILL'), 2000);
      try { await exited; } finally { clearTimeout(timer); }
    }
    if (directory) rmSync(directory, { recursive: true, force: true });
  });
  async function withVm(deny: boolean, run: (kernel: ReturnType<typeof createKernel>) => Promise<void>) {
    const filesystem = createInMemoryFileSystem();
    const kernel = createKernel({ filesystem, permissions: deny ? { network: 'deny' } : allowAll, loopbackExemptPorts: deny ? [] : [port] });
    try {
      await kernel.mount(createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }));
      await filesystem.mkdir('/tmp', { recursive: true });
      await kernel.writeFile('/tmp/ca.pem', readFileSync(join(directory, 'cert.pem')));
      await kernel.writeFile('/tmp/payload', payload);
      await run(kernel);
    } finally { await kernel.dispose(); }
  }
  it('advertises every required real protocol backend', async () => withVm(false, async kernel => {
    const result = await kernel.exec('curl --version');
    expect(result.exitCode).toBe(0);
    for (const token of ['HTTP2', 'HTTP3', 'OpenSSL/', 'nghttp2/', 'nghttp3/', 'ngtcp2/', 'libssh2/', 'scp', 'sftp']) expect(result.stdout).toContain(token);
  }), 30000);
  it('negotiates HTTP/2 and transfers exact binary bytes in both directions', async () => withVm(false, async kernel => {
    const download = await kernel.exec(`curl --http2 --cacert /tmp/ca.pem --max-time 5 -sS -o /tmp/download -w '%{http_version}' https://127.0.0.1:${port}/`);
    expect(download.exitCode).toBe(0); expect(download.stdout).toBe('2');
    expect(Buffer.from(await kernel.readFile('/tmp/download'))).toEqual(payload);
    const upload = await kernel.exec(`curl --http2 --cacert /tmp/ca.pem --max-time 5 -sS -T /tmp/payload https://127.0.0.1:${port}/upload`);
    expect(upload.exitCode).toBe(0); expect(upload.stdout).toBe('matched');
  }), 30000);
  it('rejects an untrusted certificate', async () => withVm(false, async kernel => {
    const result = await kernel.exec(`curl --http2 --max-time 5 -sS https://127.0.0.1:${port}/`);
    expect(result.exitCode).toBe(60);
  }), 30000);
  it('cancels a stalled stream at its deadline', async () => withVm(false, async kernel => {
    const result = await kernel.exec(`curl --http2 --cacert /tmp/ca.pem --max-time 1 -sS https://127.0.0.1:${port}/stall`);
    expect(result.exitCode).toBe(28);
  }), 30000);
  it('enforces network deny despite a valid certificate', async () => withVm(true, async kernel => {
    const result = await kernel.exec(`curl --http2 --cacert /tmp/ca.pem --max-time 2 -sS https://127.0.0.1:${port}/`);
    expect(result.exitCode).not.toBe(0); expect(result.stdout).toBe('');
  }), 30000);
});
