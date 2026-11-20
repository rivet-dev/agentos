import { beforeAll, afterAll, describe, it, expect } from 'vitest';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createServer, type Server } from 'node:http';
import { once } from 'node:events';
import type { ChildProcess } from 'node:child_process';
import { nativeExec, payload, quote, unusedPort, startFixture, stopFixture, ready, withVm } from './fixtures/protocol-harness';

describe('curl real ngtcp2 HTTP/3 parity', () => {
  let directory: string, port: number;
  let server: ChildProcess;
  let backend: Server;
  let files: Record<string, Uint8Array>;
  const url = () => `https://127.0.0.1:${port}/payload`;
  beforeAll(async () => {
    const nghttpx = process.env.AGENTOS_NGHTTPX_HTTP3_SERVER;
    const nativeCurl = process.env.AGENTOS_NATIVE_HTTP3_CURL;
    if (!nghttpx || !nativeCurl) throw new Error('Set AGENTOS_NGHTTPX_HTTP3_SERVER to pinned upstream nghttpx with HTTP/3 and AGENTOS_NATIVE_HTTP3_CURL to a native HTTP/3 curl; this parity gate cannot skip protocol dependencies');
    directory = mkdtempSync(join(tmpdir(), 'curl-http3-'));
    writeFileSync(join(directory, 'payload'), payload);
    await nativeExec('openssl', ['req', '-new', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1', '-keyout', join(directory, 'key.pem'), '-out', join(directory, 'cert.pem')], {timeout: 10000, maxBuffer: 65536});
    backend = createServer((request, response) => {
      request.setTimeout(3000, () => request.destroy(new Error('fixture request timeout')));
      request.on('error', error => process.stderr.write(`backend request: ${error.message}\n`));
      if (request.method !== 'PUT') { response.end(payload); return; }
      const chunks: Buffer[] = []; let size = 0;
      request.on('data', (chunk: Buffer) => {
        size += chunk.length;
        if (size > payload.length) { request.destroy(new Error('fixture upload exceeded 128KiB')); return; }
        chunks.push(chunk);
      });
      request.on('end', () => {
        const matched = Buffer.concat(chunks).equals(payload);
        response.writeHead(matched ? 201 : 422); response.end(matched ? 'matched' : 'mismatch');
      });
    });
    backend.maxConnections = 8;
    backend.requestTimeout = 5000;
    backend.listen(0, '127.0.0.1'); await once(backend, 'listening');
    const backendPort = (backend.address() as {port: number}).port;
    port = await unusedPort(true);
    server = startFixture(resolve(import.meta.dirname, 'fixtures/http3-proxy.sh'), [nghttpx, String(port), join(directory, 'key.pem'), join(directory, 'cert.pem'), String(backendPort)]);
    await ready(server, async () => {
      const result = await nativeExec(nativeCurl, ['--http3-only', '--cacert', join(directory, 'cert.pem'), '--max-time', '1', '-sS', url()], {timeout: 1500, maxBuffer: 256 * 1024, encoding: 'buffer'});
      expect(result.stdout).toEqual(payload);
    });
    files = {'ca.pem': readFileSync(join(directory, 'cert.pem')), payload};
  }, 45000);
  afterAll(async () => {
    try { await stopFixture(server); }
    finally {
      try {
        if (backend) { backend.closeAllConnections(); await new Promise<void>((resolve, reject) => backend.close(error => error ? reject(error) : resolve())); }
      } finally { if (directory) rmSync(directory, {recursive: true, force: true}); }
    }
  });
  it('negotiates HTTP/3 without fallback and downloads exact binary bytes', async () => withVm([port], files, false, async kernel => {
    const result = await kernel.exec(`curl --http3-only --cacert /tmp/ca.pem --max-time 5 -sS -o /tmp/download -w '%{http_version}' ${quote(url())}`);
    expect(result.exitCode, result.stderr).toBe(0); expect(result.stdout).toBe('3');
    expect(Buffer.from(await kernel.readFile('/tmp/download'))).toEqual(payload);
  }), 30000);
  it('uploads exact bytes through the real HTTP/3 frontend', async () => withVm([port], files, false, async kernel => {
    const result = await kernel.exec(`curl --http3-only --cacert /tmp/ca.pem --max-time 5 -sS -T /tmp/payload -w ':%{http_version}' ${quote(url())}`);
    expect(result.exitCode, result.stderr).toBe(0); expect(result.stdout).toBe('matched:3');
  }), 30000);
  it('rejects an unknown certificate', async () => withVm([port], files, false, async kernel => {
    const result = await kernel.exec(`curl --http3-only --max-time 5 -sS ${quote(url())}`);
    expect(result.exitCode, result.stderr).toBe(60); expect(result.stdout).toBe('');
  }), 30000);
  it('rejects a trusted certificate for the wrong hostname', async () => withVm([port], files, false, async kernel => {
    const result = await kernel.exec(`curl --http3-only --cacert /tmp/ca.pem --resolve wrong.gameinc.io:${port}:127.0.0.1 --max-time 5 -sS https://wrong.gameinc.io:${port}/payload`);
    expect(result.exitCode, result.stderr).toBe(60); expect(result.stdout).toBe('');
  }), 30000);
  it('cancels a rate-limited QUIC transfer', async () => withVm([port], files, false, async kernel => {
    const result = await kernel.exec(`curl --http3-only --cacert /tmp/ca.pem --max-time 1 --limit-rate 1024 -sS -o /tmp/partial ${quote(url())}`);
    expect(result.exitCode, result.stderr).toBe(28);
    expect((await kernel.readFile('/tmp/partial')).length).toBeLessThan(payload.length);
  }), 30000);
  it('enforces UDP network deny', async () => withVm([port], files, true, async kernel => {
    const result = await kernel.exec(`curl --http3-only --cacert /tmp/ca.pem --max-time 2 -sS ${quote(url())}`);
    expect(result.exitCode).not.toBe(0); expect(result.stdout).toBe('');
  }), 30000);
});
