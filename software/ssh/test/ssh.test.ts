/**
 * Integration tests for the real OpenSSH ssh client (10.4p1, built
 * --without-openssl: ed25519 + curve25519-sha256 + chacha20-poly1305).
 *
 * Mirrors the git HTTPS suite's loopback-server harness: an in-test SSH
 * server (the pure-JS `ssh2` package) listens on a host loopback port that
 * the kernel exempts, and the WASM ssh client connects out through the
 * kernel's host_net path. Covers batch/key-based exec, host-key
 * verification (known_hosts, StrictHostKeyChecking=accept-new), publickey
 * auth failure, and git-over-ssh (clone + push against host
 * git-upload-pack/git-receive-pack).
 */

import { describe, it, expect, afterEach, beforeAll, afterAll, vi } from 'vitest';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawn, spawnSync } from 'node:child_process';
import { Server as SshServer, utils as sshUtils } from 'ssh2';
import type { Connection } from 'ssh2';
import { createWasmVmRuntime } from '@agentos/test-harness';
import {
  allowAll,
  COMMANDS_DIR,
  createInMemoryFileSystem,
  createKernel,
  describeIf,
  hasWasmBinaries,
} from '@agentos/test-harness';
import type { Kernel } from '@agentos/test-harness';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 60_000 });

const hasSsh = hasWasmBinaries && existsSync(resolve(COMMANDS_DIR, 'ssh'));
const hasGit = hasWasmBinaries && existsSync(resolve(COMMANDS_DIR, 'git'));
const hasHostGit = spawnSync('git', ['--version'], { stdio: 'ignore' }).status === 0;

const SSH_USER = 'agentos';

interface TestKeys {
  hostKey: ReturnType<typeof sshUtils.generateKeyPairSync>;
  clientKey: ReturnType<typeof sshUtils.generateKeyPairSync>;
  /** A second client keypair the server does NOT authorize. */
  wrongClientKey: ReturnType<typeof sshUtils.generateKeyPairSync>;
  /** A second host key used to simulate a changed/unknown server identity. */
  otherHostKey: ReturnType<typeof sshUtils.generateKeyPairSync>;
}

function generateKeys(): TestKeys {
  return {
    hostKey: sshUtils.generateKeyPairSync('ed25519'),
    clientKey: sshUtils.generateKeyPairSync('ed25519'),
    wrongClientKey: sshUtils.generateKeyPairSync('ed25519'),
    otherHostKey: sshUtils.generateKeyPairSync('ed25519'),
  };
}

/** Standard ssh2 publickey-auth handler restricted to one authorized key. */
function installAuthHandler(client: Connection, authorizedPublicKey: string) {
  const allowed = sshUtils.parseKey(authorizedPublicKey);
  if (allowed instanceof Error) throw allowed;
  client.on('authentication', (ctx) => {
    if (ctx.method !== 'publickey') {
      return ctx.reject(['publickey']);
    }
    const matches =
      ctx.key.algo === allowed.type &&
      ctx.key.data.equals(allowed.getPublicSSH());
    if (!matches) {
      return ctx.reject(['publickey']);
    }
    if (ctx.signature && ctx.blob) {
      if (allowed.verify(ctx.blob, ctx.signature, ctx.hashAlgo) === true) {
        return ctx.accept();
      }
      return ctx.reject(['publickey']);
    }
    // pk-check phase (no signature yet): tell the client the key is OK.
    return ctx.accept();
  });
}

/** exec handler: `echo hello`-style canned command execution. */
function installEchoExecHandler(client: Connection) {
  client.on('ready', () => {
    client.on('session', (acceptSession) => {
      const session = acceptSession();
      session.on('exec', (acceptExec, _reject, info) => {
        const stream = acceptExec();
        if (info.command === 'echo hello') {
          stream.write('hello\n');
          stream.exit(0);
        } else {
          stream.stderr.write(`unknown test command: ${info.command}\n`);
          stream.exit(127);
        }
        stream.end();
      });
    });
  });
}

/**
 * exec handler bridging `git-upload-pack '/x.git'` / `git-receive-pack ...`
 * to the host git against a bare repo root — an in-test stand-in for a real
 * SSH git host (what git-shell does on a server).
 */
function installGitExecHandler(client: Connection, repoRoot: string) {
  client.on('ready', () => {
    client.on('session', (acceptSession) => {
      const session = acceptSession();
      session.on('exec', (acceptExec, reject, info) => {
        const match = /^(git-upload-pack|git-receive-pack|git-upload-archive) '(.*)'$/.exec(
          info.command,
        );
        if (!match) {
          const stream = acceptExec();
          stream.stderr.write(`unsupported command: ${info.command}\n`);
          stream.exit(128);
          stream.end();
          return;
        }
        const [, service, requestedPath] = match;
        const repoPath = join(repoRoot, requestedPath.replace(/^\/+/, ''));
        const stream = acceptExec();
        const child = spawn('git', [service.replace(/^git-/, ''), repoPath]);
        stream.pipe(child.stdin);
        child.stdout.pipe(stream, { end: false });
        child.stderr.pipe(stream.stderr, { end: false });
        child.on('close', (code) => {
          stream.exit(code ?? 1);
          stream.end();
        });
        child.on('error', () => {
          stream.exit(127);
          stream.end();
        });
      });
    });
  });
}

async function listen(server: SshServer): Promise<number> {
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  return (server.address() as import('node:net').AddressInfo).port;
}

async function createSshKernel(loopbackExemptPorts: number[]) {
  const vfs = createInMemoryFileSystem();
  await (vfs as any).chmod('/', 0o1777);
  await vfs.mkdir('/tmp', { recursive: true });
  await (vfs as any).chmod('/tmp', 0o1777);
  const kernel = createKernel({
    filesystem: vfs,
    permissions: allowAll,
    loopbackExemptPorts,
    syncFilesystemOnDispose: false,
  });
  await kernel.mount(createWasmVmRuntime({ commandDirs: [COMMANDS_DIR] }));
  return { kernel, vfs, dispose: () => kernel.dispose() };
}

async function run(
  kernel: Kernel,
  cmd: string,
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const r = await kernel.exec(cmd);
  if (r.exitCode !== 0) {
    throw new Error(
      `Command failed (exit ${r.exitCode}): ${cmd}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`,
    );
  }
  return r;
}

/**
 * Resolve the guest user's home directory (ssh resolves `~` through
 * getpwuid(getuid())->pw_dir, which the runtime keeps aligned with $HOME).
 */
async function guestHome(kernel: Kernel): Promise<string> {
  const r = await run(kernel, "sh -c 'echo $HOME'");
  const home = r.stdout.trim();
  expect(home).toMatch(/^\//);
  return home;
}

/** Seed ~/.ssh with an identity and (optionally) a known_hosts line. */
async function seedSshDir(
  kernel: Kernel,
  vfs: any,
  home: string,
  privateKey: string,
  knownHostsLine?: string,
): Promise<string> {
  const sshDir = `${home}/.ssh`;
  await vfs.mkdir(sshDir, { recursive: true });
  await vfs.chmod(sshDir, 0o700);
  await kernel.writeFile(`${sshDir}/id_ed25519`, `${privateKey}\n`);
  await vfs.chmod(`${sshDir}/id_ed25519`, 0o600);
  if (knownHostsLine !== undefined) {
    await kernel.writeFile(`${sshDir}/known_hosts`, `${knownHostsLine}\n`);
    await vfs.chmod(`${sshDir}/known_hosts`, 0o600);
  }
  return sshDir;
}

function knownHostsEntry(port: number, hostPublicKey: string): string {
  // `[host]:port` hashing syntax from sshd(8) AUTHORIZED_KEYS/known_hosts
  // format; non-default ports always use the bracketed form.
  return `[127.0.0.1]:${port} ${hostPublicKey}`;
}

// TODO(P6): requires the ssh WASM artifact, intentionally excluded from the
// fast software-build gate (same as git).
describeIf(hasSsh, 'ssh command', () => {
  let kernel: Kernel;
  let vfs: any;
  let dispose: (() => Promise<void>) | undefined;

  afterEach(async () => {
    await dispose?.();
    dispose = undefined;
  });

  it('ssh -V reports the real OpenSSH version without OpenSSL', async () => {
    ({ kernel, vfs, dispose } = await createSshKernel([]));
    const r = await kernel.exec('ssh -V');
    expect(r.exitCode).toBe(0);
    const banner = `${r.stdout}${r.stderr}`;
    expect(banner).toMatch(/OpenSSH_10\.4/);
    expect(banner).toMatch(/without OpenSSL/i);
  });

  describe('against an in-test ssh2 server', () => {
    let keys: TestKeys;
    let server: SshServer;
    let port: number;

    beforeAll(async () => {
      keys = generateKeys();
      server = new SshServer({ hostKeys: [keys.hostKey.private] }, (client) => {
        installAuthHandler(client, keys.clientKey.public);
        installEchoExecHandler(client);
      });
      port = await listen(server);
    });

    afterAll(async () => {
      await new Promise<void>((r) => server.close(() => r()));
    });

    const sshCmd = (extra: string) =>
      `ssh -T -o BatchMode=yes ${extra} -p ${port} ${SSH_USER}@127.0.0.1 echo hello`;

    it('runs a remote command with ed25519 publickey auth and known_hosts', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(
        kernel,
        vfs,
        home,
        keys.clientKey.private,
        knownHostsEntry(port, keys.hostKey.public),
      );

      const r = await kernel.exec(sshCmd(''));
      expect(r.stderr).not.toMatch(/setsockopt/i);
      expect(r.stdout).toBe('hello\n');
      expect(r.exitCode).toBe(0);
    });

    it('propagates the remote exit status', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(
        kernel,
        vfs,
        home,
        keys.clientKey.private,
        knownHostsEntry(port, keys.hostKey.public),
      );

      const r = await kernel.exec(
        `ssh -T -o BatchMode=yes -p ${port} ${SSH_USER}@127.0.0.1 false-command`,
      );
      expect(r.exitCode).toBe(127);
      expect(r.stderr).toContain('unknown test command');
    });

    it('fails publickey auth with an unauthorized client key', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(
        kernel,
        vfs,
        home,
        keys.wrongClientKey.private,
        knownHostsEntry(port, keys.hostKey.public),
      );

      const r = await kernel.exec(sshCmd(''));
      expect(r.exitCode).not.toBe(0);
      expect(r.stderr).toMatch(/Permission denied \(publickey\)/i);
      expect(r.stdout).not.toContain('hello');
    });

    it('fails host key verification when known_hosts pins a different key', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(
        kernel,
        vfs,
        home,
        keys.clientKey.private,
        knownHostsEntry(port, keys.otherHostKey.public),
      );

      const r = await kernel.exec(sshCmd(''));
      expect(r.exitCode).not.toBe(0);
      expect(r.stderr).toMatch(
        /REMOTE HOST IDENTIFICATION HAS CHANGED|Host key verification failed/i,
      );
      expect(r.stdout).not.toContain('hello');
    });

    it('fails closed in BatchMode when the host key is unknown', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(kernel, vfs, home, keys.clientKey.private);

      const r = await kernel.exec(sshCmd(''));
      expect(r.exitCode).not.toBe(0);
      expect(r.stderr).toMatch(/Host key verification failed/i);
    });

    it('StrictHostKeyChecking=accept-new succeeds and records the host key', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      const sshDir = await seedSshDir(kernel, vfs, home, keys.clientKey.private);

      const r = await kernel.exec(sshCmd('-o StrictHostKeyChecking=accept-new'));
      expect(r.stdout).toBe('hello\n');
      expect(r.exitCode).toBe(0);
      expect(r.stderr).toMatch(/Permanently added/i);

      const knownHosts = new TextDecoder().decode(
        await kernel.readFile(`${sshDir}/known_hosts`),
      );
      const hostKeyBlob = keys.hostKey.public.split(/\s+/)[1];
      expect(knownHosts).toContain(`[127.0.0.1]:${port}`);
      expect(knownHosts).toContain(hostKeyBlob);
    });
  });

  // git-over-ssh: the WASM git execs the WASM ssh from PATH (git connect.c),
  // which tunnels git-upload-pack / git-receive-pack to the host-side bare
  // repo behind the ssh2 server.
  //
  // This also regresses mixed polling in the runtime: ssh polls a dup'd stdin
  // pipe alongside its host-net socket while Git waits for the remote helper.
  describeIf(hasGit && hasHostGit, 'git-over-ssh clone/push', () => {
    let keys: TestKeys;
    let server: SshServer;
    let port: number;
    let repoRoot: string;

    const gitConfig = [
      '-c safe.directory=*',
      '-c init.defaultBranch=main',
      '-c user.name=agentos',
      '-c user.email=agentos@example.invalid',
    ].join(' ');
    const git = (args: string) => `git ${gitConfig} ${args}`;

    function runHostGit(args: string[], cwd?: string) {
      const result = spawnSync('git', args, { cwd, encoding: 'utf8' });
      if (result.status !== 0) {
        throw new Error(
          `host git failed: git ${args.join(' ')}\nstdout: ${result.stdout}\nstderr: ${result.stderr}`,
        );
      }
    }

    beforeAll(async () => {
      keys = generateKeys();
      repoRoot = mkdtempSync(join(tmpdir(), 'agentos-git-ssh-'));
      const worktree = join(repoRoot, 'worktree');
      const origin = join(repoRoot, 'origin.git');

      runHostGit(['-c', 'init.defaultBranch=main', 'init', worktree]);
      writeFileSync(join(worktree, 'README.md'), 'remote ssh clone\n');
      runHostGit(['-C', worktree, 'add', 'README.md']);
      runHostGit([
        '-C', worktree,
        '-c', 'user.name=agentos', '-c', 'user.email=agentos@example.invalid',
        'commit', '-m', 'seed',
      ]);
      runHostGit(['clone', '--bare', worktree, origin]);

      server = new SshServer({ hostKeys: [keys.hostKey.private] }, (client) => {
        installAuthHandler(client, keys.clientKey.public);
        installGitExecHandler(client, repoRoot);
      });
      port = await listen(server);
    });

    afterAll(async () => {
      if (server) await new Promise<void>((r) => server.close(() => r()));
      rmSync(repoRoot, { recursive: true, force: true });
    });

    it('clones and pushes over ssh://', async () => {
      ({ kernel, vfs, dispose } = await createSshKernel([port]));
      const home = await guestHome(kernel);
      await seedSshDir(
        kernel,
        vfs,
        home,
        keys.clientKey.private,
        knownHostsEntry(port, keys.hostKey.public),
      );

      const url = `ssh://${SSH_USER}@127.0.0.1:${port}/origin.git`;

      const cloned = await kernel.exec(git(`clone ${url} /tmp/clone`));
      expect(cloned.exitCode, cloned.stderr).toBe(0);
      const readme = new TextDecoder().decode(
        await kernel.readFile('/tmp/clone/README.md'),
      );
      expect(readme).toBe('remote ssh clone\n');
      const head = new TextDecoder().decode(
        await kernel.readFile('/tmp/clone/.git/HEAD'),
      );
      expect(head.trim()).toBe('ref: refs/heads/main');

      // Push a new commit back over the same transport.
      await kernel.writeFile('/tmp/clone/pushed.txt', 'pushed over ssh\n');
      await run(kernel, git('-C /tmp/clone add pushed.txt'));
      await run(kernel, git("-C /tmp/clone commit -m 'push over ssh'"));
      const pushed = await kernel.exec(
        git('-C /tmp/clone push origin HEAD:refs/heads/ssh-push'),
      );
      expect(pushed.exitCode).toBe(0);

      // Verify the ref really landed in the host-side bare repo.
      const originRef = spawnSync(
        'git',
        ['-C', join(repoRoot, 'origin.git'), 'rev-parse', '--verify', 'refs/heads/ssh-push'],
        { encoding: 'utf8' },
      );
      expect(originRef.status).toBe(0);
      expect(originRef.stdout.trim()).toMatch(/^[0-9a-f]{40,64}$/);
    });
  });
});
