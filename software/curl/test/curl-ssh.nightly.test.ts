import { beforeAll, afterAll, describe, it, expect } from 'vitest';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { homedir, userInfo } from 'node:os';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import type { ChildProcess } from 'node:child_process';
import { nativeExec, payload, quote, unusedPort, startFixture, stopFixture, ready, withVm } from './fixtures/protocol-harness';

describe('curl real libssh2 SFTP/SCP parity', () => {
  let directory: string, port: number, fingerprint: string;
  let server: ChildProcess;
  let files: Record<string, Uint8Array>;
  const username = userInfo().username;
  const nativeCurl = process.env.AGENTOS_NATIVE_SSH_CURL ?? 'curl';
  const url = (scheme: string, file: string) => `${scheme}://127.0.0.1:${port}${directory}/${file}`;
  const auth = () => `--user ${quote(username + ':')} --key /tmp/client_key --pubkey /tmp/client_key.pub --hostpubsha256 ${quote(fingerprint)}`;
  beforeAll(async () => {
    // StrictModes validates every ancestor up to the user's home; /tmp is writable.
    directory = mkdtempSync(join(homedir(), '.agentos-curl-ssh-'));
    writeFileSync(join(directory, 'payload'), payload);
    port = await unusedPort(false);
    server = startFixture(resolve(import.meta.dirname, 'fixtures/ssh-server.sh'), [directory, String(port)]);
    await ready(server, async () => {
      const publicKey = readFileSync(join(directory, 'host_key.pub'), 'utf8').trim().split(/\s+/)[1];
      fingerprint = createHash('sha256').update(Buffer.from(publicKey, 'base64')).digest('base64');
      const result = await nativeExec(nativeCurl, ['--max-time', '1', '--silent', '--show-error', '--user', username + ':', '--key', join(directory, 'client_key'), '--pubkey', join(directory, 'client_key.pub'), '--hostpubsha256', fingerprint, url('sftp', 'payload')], {timeout: 1500, maxBuffer: 256 * 1024, encoding: 'buffer'});
      expect(result.stdout).toEqual(payload);
    });
    files = {'payload': payload, 'client_key': readFileSync(join(directory, 'client_key')), 'client_key.pub': readFileSync(join(directory, 'client_key.pub'))};
  }, 40000);
  afterAll(async () => { try { await stopFixture(server); } finally { if (directory) rmSync(directory, {recursive: true, force: true}); } });
  for (const scheme of ['sftp', 'scp']) {
    it(`${scheme} downloads and uploads exact bytes with verified host key`, async () => withVm([port], files, false, async kernel => {
      const download = await kernel.exec(`curl -sS --max-time 5 ${auth()} -o /tmp/download ${quote(url(scheme, 'payload'))}`);
      expect(download.exitCode).toBe(0); expect(Buffer.from(await kernel.readFile('/tmp/download'))).toEqual(payload);
      const upload = await kernel.exec(`curl -sS --max-time 5 ${auth()} -T /tmp/payload ${quote(url(scheme, `upload-${scheme}`))}`);
      expect(upload.exitCode).toBe(0); expect(readFileSync(join(directory, `upload-${scheme}`))).toEqual(payload);
    }), 30000);
    it(`${scheme} rejects a mismatched host key before transferring bytes`, async () => withVm([port], files, false, async kernel => {
      const wrong = Buffer.alloc(32, 1).toString('base64');
      const result = await kernel.exec(`curl -sS --max-time 5 ${auth().replace(quote(fingerprint), quote(wrong))} ${quote(url(scheme, 'payload'))}`);
      expect(result.exitCode).toBe(60); expect(result.stdout).toBe('');
    }), 30000);
    it(`${scheme} rejects incorrect authentication`, async () => withVm([port], files, false, async kernel => {
      const result = await kernel.exec(`curl -sS --max-time 5 --user ${quote(username + ':invalid-test-password')} --hostpubsha256 ${quote(fingerprint)} ${quote(url(scheme, 'payload'))}`);
      expect(result.exitCode).toBe(67); expect(result.stdout).toBe('');
    }), 30000);
    it(`${scheme} cancels a rate-limited real transfer`, async () => withVm([port], files, false, async kernel => {
      const result = await kernel.exec(`curl -sS --max-time 1 --limit-rate 1024 ${auth()} -o /tmp/partial ${quote(url(scheme, 'payload'))}`);
      expect(result.exitCode).toBe(28);
      expect((await kernel.readFile('/tmp/partial')).length).toBeLessThan(payload.length);
    }), 30000);
    it(`${scheme} obeys network deny`, async () => withVm([port], files, true, async kernel => {
      const result = await kernel.exec(`curl -sS --max-time 2 ${auth()} ${quote(url(scheme, 'payload'))}`);
      expect(result.exitCode).not.toBe(0); expect(result.stdout).toBe('');
    }), 30000);
  }
});
