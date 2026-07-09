/**
 * Integration tests for git command.
 *
 * Verifies init, add, commit, branch, checkout (with DWIM), plus local and
 * smart-HTTP remote clone via kernel.exec() with real WASM binaries.
 */

import { describe, it, expect, afterEach, beforeAll, afterAll, vi } from 'vitest';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import { createServer as createHttpsServer, type Server as HttpsServer } from 'node:https';
import { spawn, spawnSync, execSync } from 'node:child_process';
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

vi.setConfig({ testTimeout: 30_000 });

/** Check git binary exists in addition to base WASM binaries */
const hasGit = hasWasmBinaries && existsSync(resolve(COMMANDS_DIR, 'git'));
const hasHostGit = spawnSync('git', ['--version'], { stdio: 'ignore' }).status === 0;
// Smart HTTP needs Git's libcurl-backed remote helper. It is now a real second
// WASM binary (git-remote-http links the overlaid mbedTLS libcurl in-process);
// git-remote-https aliases to it.
const hasGitHttpHelper =
  hasGit && existsSync(resolve(COMMANDS_DIR, 'git-remote-http'));

const gitConfig = [
  '-c safe.directory=*',
  '-c init.defaultBranch=main',
  '-c user.name=agentos',
  '-c user.email=agentos@example.invalid',
].join(' ');

function git(args: string) {
  return `git ${gitConfig} ${args}`;
}

/** Create a kernel with a world-writable in-memory filesystem */
async function createGitKernel() {
  const vfs = createInMemoryFileSystem();
  // Make root and /tmp writable by all users (WASM processes run as non-root)
  await (vfs as any).chmod('/', 0o1777);
  await vfs.mkdir('/tmp', { recursive: true });
  await (vfs as any).chmod('/tmp', 0o1777);
  const kernel = createKernel({ filesystem: vfs, syncFilesystemOnDispose: false });
  await kernel.mount(createWasmVmRuntime({ commandDirs: [COMMANDS_DIR] }));
  return { kernel, vfs, dispose: () => kernel.dispose() };
}

async function createGitKernelWithNet(loopbackExemptPorts: number[], seededCaPem?: string) {
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
  // Seed the Debian-shaped trust store the way the native VM bootstrap does, so
  // libcurl's compile-time default CA bundle (/etc/ssl/certs/ca-certificates.crt)
  // resolves in-guest for git-remote-http's mbedTLS backend.
  if (seededCaPem) {
    await vfs.mkdir('/etc/ssl/certs', { recursive: true });
    await kernel.writeFile('/etc/ssl/certs/ca-certificates.crt', seededCaPem);
  }
  return { kernel, vfs, dispose: () => kernel.dispose() };
}

// Build a real CA and a leaf server certificate signed by it, with a SAN that
// covers the 127.0.0.1 loopback endpoint the VM connects to. This lets
// git-remote-http's mbedTLS backend perform genuine chain + hostname
// verification, exactly like Linux git against a private CA.
function makeCaSignedCert(caCommonName: string): {
  caPem: string;
  serverKey: string;
  serverCert: string;
} {
  const dir = mkdtempSync(join(tmpdir(), 'git-ca-'));
  try {
    execSync(`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "${dir}/ca.key" 2>/dev/null`);
    execSync(
      `openssl req -x509 -new -key "${dir}/ca.key" -days 3650 -subj "/CN=${caCommonName}" -out "${dir}/ca.crt" 2>/dev/null`,
    );
    execSync(`openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "${dir}/srv.key" 2>/dev/null`);
    execSync(`openssl req -new -key "${dir}/srv.key" -subj "/CN=localhost" -out "${dir}/srv.csr" 2>/dev/null`);
    writeFileSync(`${dir}/ext.cnf`, 'subjectAltName=DNS:localhost,IP:127.0.0.1\n');
    execSync(
      `openssl x509 -req -in "${dir}/srv.csr" -CA "${dir}/ca.crt" -CAkey "${dir}/ca.key" ` +
      `-CAcreateserial -days 3650 -extfile "${dir}/ext.cnf" -out "${dir}/srv.crt" 2>/dev/null`,
    );
    return {
      caPem: readFileSync(`${dir}/ca.crt`, 'utf8'),
      serverKey: readFileSync(`${dir}/srv.key`, 'utf8'),
      serverCert: readFileSync(`${dir}/srv.crt`, 'utf8'),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function runHostGit(args: string[], cwd?: string) {
  const result = spawnSync('git', args, {
    cwd,
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    throw new Error(
      `host git failed: git ${args.join(' ')}\nstdout: ${result.stdout}\nstderr: ${result.stderr}`,
    );
  }
}

/** Helper: run command and assert success */
async function run(kernel: Kernel, cmd: string): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const r = await kernel.exec(cmd);
  if (r.exitCode !== 0) {
    throw new Error(`Command failed (exit ${r.exitCode}): ${cmd}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`);
  }
  return r;
}

async function expectGitRef(kernel: Kernel, repo: string, ref: string) {
  const result = await run(kernel, git(`-C ${repo} rev-parse --verify ${ref}`));
  expect(result.stdout.trim()).toMatch(/^[0-9a-f]{40,64}$/);
}

// TODO(P6): requires git WASM artifact, intentionally excluded from the fast software-build gate.
describeIf(hasGit, 'git command', () => {
  let kernel: Kernel;
  let vfs: any;
  let dispose: () => Promise<void>;

  afterEach(async () => {
    await dispose?.();
  });

  it('init creates .git directory structure', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    const result = await run(kernel, git('init /repo'));
    expect(result.stdout).toContain('Initialized empty Git repository');

    expect(await vfs.exists('/repo/.git/HEAD')).toBe(true);
    expect(await vfs.exists('/repo/.git/objects')).toBe(true);
    expect(await vfs.exists('/repo/.git/refs/heads')).toBe(true);

    const head = new TextDecoder().decode(await vfs.readFile('/repo/.git/HEAD'));
    expect(head.trim()).toBe('ref: refs/heads/main');
  });

  it('add + commit creates objects and updates ref', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /repo'));
    await kernel.writeFile('/repo/hello.txt', 'hello world\n');
    await run(kernel, git('-C /repo add hello.txt'));
    await run(kernel, git("-C /repo commit -m 'first commit'"));

    expect(await vfs.exists('/repo/.git/refs/heads/main')).toBe(true);
  });

  it('branch lists branches with current marked', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /repo'));
    await kernel.writeFile('/repo/file.txt', 'content\n');
    await run(kernel, git('-C /repo add file.txt'));
    await run(kernel, git("-C /repo commit -m 'init'"));

    const result = await run(kernel, git('-C /repo branch'));
    expect(result.stdout.trim()).toBe('* main');
  });

  it('checkout -b creates a new branch', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /repo'));
    await kernel.writeFile('/repo/file.txt', 'content\n');
    await run(kernel, git('-C /repo add file.txt'));
    await run(kernel, git("-C /repo commit -m 'init'"));

    await run(kernel, git('-C /repo checkout -b feature'));

    const result = await run(kernel, git('-C /repo branch'));
    const lines = result.stdout.trim().split('\n').map((l: string) => l.trim());
    expect(lines).toContain('* feature');
    expect(lines).toContain('main');
  });

  it('full quickstart scenario: init, commit, branch, clone, checkout', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    // Create origin repo
    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', '# demo repo\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'initial commit'"));

    // Check default branch
    let r = await run(kernel, git('-C /tmp/origin branch'));
    expect(r.stdout.trim()).toBe('* main');

    // Create feature branch with a new file
    await run(kernel, git('-C /tmp/origin checkout -b feature'));
    await kernel.writeFile('/tmp/origin/feature.txt', 'checked out from feature\n');
    await run(kernel, git('-C /tmp/origin add feature.txt'));
    await run(kernel, git("-C /tmp/origin commit -m 'add feature file'"));

    // Switch back to main
    await run(kernel, git('-C /tmp/origin checkout main'));

    // Clone
    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    // Clone should only show main branch initially
    r = await run(kernel, git('-C /tmp/clone branch'));
    expect(r.stdout.trim()).toBe('* main');

    // Checkout feature (DWIM from remote tracking)
    await run(kernel, git('-C /tmp/clone checkout feature'));

    // Now both branches should be listed
    r = await run(kernel, git('-C /tmp/clone branch'));
    const branches = r.stdout.trim().split('\n').map((l: string) => l.trim());
    expect(branches).toContain('* feature');
    expect(branches).toContain('main');

    // Verify feature file exists in clone
    const featureContent = new TextDecoder().decode(await vfs.readFile('/tmp/clone/feature.txt'));
    expect(featureContent).toBe('checked out from feature\n');

    // Verify README exists too
    const readmeContent = new TextDecoder().decode(await vfs.readFile('/tmp/clone/README.md'));
    expect(readmeContent).toBe('# demo repo\n');
  });

  it('clone without an explicit destination uses the source basename', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', 'default destination\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'seed'"));

    await run(kernel, 'mkdir -p /work');
    await run(kernel, git('-C /work clone /tmp/origin'));

    expect(await vfs.exists('/work/origin/.git/HEAD')).toBe(true);
    const readmeContent = new TextDecoder().decode(await vfs.readFile('/work/origin/README.md'));
    expect(readmeContent).toBe('default destination\n');
  });

  it('clone without an explicit destination strips a trailing .git suffix', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin.git'));
    await kernel.writeFile('/tmp/origin.git/README.md', 'suffix destination\n');
    await run(kernel, git('-C /tmp/origin.git add README.md'));
    await run(kernel, git("-C /tmp/origin.git commit -m 'seed'"));

    await run(kernel, 'mkdir -p /work');
    await run(kernel, git('-C /work clone /tmp/origin.git'));

    expect(await vfs.exists('/work/origin/.git/HEAD')).toBe(true);
    const readmeContent = new TextDecoder().decode(await vfs.readFile('/work/origin/README.md'));
    expect(readmeContent).toBe('suffix destination\n');
  });

  it('clone into an existing empty destination directory succeeds', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', 'empty destination\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'seed'"));

    await run(kernel, 'mkdir -p /tmp/clone');
    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    expect(await vfs.exists('/tmp/clone/.git/HEAD')).toBe(true);
    const readmeContent = new TextDecoder().decode(await vfs.readFile('/tmp/clone/README.md'));
    expect(readmeContent).toBe('empty destination\n');
  });

  it('clone rejects a non-empty destination directory', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', 'origin\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'seed'"));

    await run(kernel, 'mkdir -p /tmp/clone');
    await kernel.writeFile('/tmp/clone/existing.txt', 'keep me\n');

    const result = await kernel.exec(git('clone /tmp/origin /tmp/clone'));
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr).toMatch(/already exists|not an empty directory|destination/i);

    const existing = new TextDecoder().decode(await vfs.readFile('/tmp/clone/existing.txt'));
    expect(existing).toBe('keep me\n');
    expect(await vfs.exists('/tmp/clone/.git')).toBe(false);
  });

  it('clone of a missing repository fails without leaving a partial destination', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    const result = await kernel.exec(git('clone /tmp/missing /tmp/clone'));
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr).toMatch(/not a git repository|missing|no such file|fatal/i);
    expect(await vfs.exists('/tmp/clone')).toBe(false);
  });

  it('clone of an empty repository succeeds and leaves an empty worktree', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    const head = new TextDecoder().decode(await vfs.readFile('/tmp/clone/.git/HEAD'));
    expect(head.trim()).toBe('ref: refs/heads/main');
    expect(await vfs.exists('/tmp/clone/.git/config')).toBe(true);
    expect(await vfs.exists('/tmp/clone/.git/refs/heads/main')).toBe(false);
    expect(await vfs.exists('/tmp/clone/README.md')).toBe(false);
  });

  it('clone preserves nested directory trees', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await run(kernel, 'mkdir -p /tmp/origin/src/nested');
    await kernel.writeFile('/tmp/origin/src/nested/file.txt', 'nested payload\n');
    await kernel.writeFile('/tmp/origin/src/root.txt', 'root payload\n');
    await run(kernel, git('-C /tmp/origin add src/nested/file.txt src/root.txt'));
    await run(kernel, git("-C /tmp/origin commit -m 'nested tree'"));

    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    const nested = new TextDecoder().decode(await vfs.readFile('/tmp/clone/src/nested/file.txt'));
    const root = new TextDecoder().decode(await vfs.readFile('/tmp/clone/src/root.txt'));
    expect(nested).toBe('nested payload\n');
    expect(root).toBe('root payload\n');
  });

  it('clone honors the source default branch when HEAD is not main', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', 'main branch\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'main'"));

    await run(kernel, git('-C /tmp/origin checkout -b trunk'));
    await kernel.writeFile('/tmp/origin/trunk.txt', 'trunk branch\n');
    await run(kernel, git('-C /tmp/origin add trunk.txt'));
    await run(kernel, git("-C /tmp/origin commit -m 'trunk'"));

    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    const head = new TextDecoder().decode(await vfs.readFile('/tmp/clone/.git/HEAD'));
    expect(head.trim()).toBe('ref: refs/heads/trunk');
    expect(await vfs.exists('/tmp/clone/.git/refs/heads/trunk')).toBe(true);
    const trunk = new TextDecoder().decode(await vfs.readFile('/tmp/clone/trunk.txt'));
    expect(trunk).toBe('trunk branch\n');
  });

  it('clone copies nested branch refs and checkout DWIM works for branch names with slashes', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/origin'));
    await kernel.writeFile('/tmp/origin/README.md', '# demo repo\n');
    await run(kernel, git('-C /tmp/origin add README.md'));
    await run(kernel, git("-C /tmp/origin commit -m 'initial commit'"));

    await run(kernel, git('-C /tmp/origin checkout -b feature/deep'));
    await kernel.writeFile('/tmp/origin/feature.txt', 'nested branch payload\n');
    await run(kernel, git('-C /tmp/origin add feature.txt'));
    await run(kernel, git("-C /tmp/origin commit -m 'nested branch'"));
    await run(kernel, git('-C /tmp/origin checkout main'));

    await run(kernel, git('clone /tmp/origin /tmp/clone'));

    await expectGitRef(kernel, '/tmp/clone', 'refs/remotes/origin/feature/deep');

    await run(kernel, git('-C /tmp/clone checkout feature/deep'));
    const featureContent = new TextDecoder().decode(await vfs.readFile('/tmp/clone/feature.txt'));
    expect(featureContent).toBe('nested branch payload\n');
    const head = new TextDecoder().decode(await vfs.readFile('/tmp/clone/.git/HEAD'));
    expect(head.trim()).toBe('ref: refs/heads/feature/deep');
  });

  it('clone works with relative source and destination paths', async () => {
    ({ kernel, vfs, dispose } = await createGitKernel());

    await run(kernel, 'mkdir -p /tmp/work');
    await run(kernel, git('init /tmp/work/origin'));
    await kernel.writeFile('/tmp/work/origin/README.md', 'relative clone\n');
    await run(kernel, git('-C /tmp/work/origin add README.md'));
    await run(kernel, git("-C /tmp/work/origin commit -m 'seed'"));

    await run(kernel, git('-C /tmp/work clone ./origin ./clone'));

    expect(await vfs.exists('/tmp/work/clone/.git/HEAD')).toBe(true);
    const readmeContent = new TextDecoder().decode(await vfs.readFile('/tmp/work/clone/README.md'));
    expect(readmeContent).toBe('relative clone\n');
  });

  it('push fails with a real Git remote/ref error', async () => {
    ({ kernel, dispose } = await createGitKernel());

    await run(kernel, git('init /tmp/repo'));

    const result = await kernel.exec(git('-C /tmp/repo push origin main'));
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr).toMatch(/fatal|error|origin|refspec|repository/i);
    expect(result.stderr).not.toContain('GitSubcommandUnsupported');
  });

  it('clone rejects SSH-style remotes with a real Git spawn failure', async () => {
    ({ kernel, dispose } = await createGitKernel());

    const result = await kernel.exec(git('clone git@github.com:rivet-dev/agentos.git /tmp/clone'));
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr).toMatch(/cannot run ssh|unable to fork|ssh|fatal/i);
    expect(result.stderr).not.toContain('GitSubcommandUnsupported');
  });

  // Real smart-HTTP over TLS: git-remote-http (libcurl + in-guest mbedTLS)
  // clones/fetches/pushes against `git http-backend` behind a Node HTTPS
  // endpoint. Certificate trust comes from a private CA seeded into the guest's
  // /etc/ssl/certs bundle (the "trusted" server) — exactly the Debian trust
  // path — while a second server signed by a CA absent from the bundle exercises
  // verify-fail, http.sslVerify=false, GIT_SSL_NO_VERIFY, and http.sslCAInfo.
  describeIf(hasHostGit && hasGitHttpHelper, 'smart-HTTP clone/fetch/push over TLS', () => {
    let repoRoot: string;
    let trustedServer: HttpsServer;
    let untrustedServer: HttpsServer;
    let trustedPort: number;
    let untrustedPort: number;
    let trustedCaPem = '';
    let untrustedCaPem = '';

    // A CGI bridge to `git http-backend`. receive-pack is enabled on the origin
    // (below) so pushes are accepted; GIT_HTTP_EXPORT_ALL allows anonymous read.
    function makeBackendHandler() {
      return (req: import('node:http').IncomingMessage, res: import('node:http').ServerResponse) => {
        const url = new URL(req.url ?? '/', 'https://127.0.0.1');
        const bodyChunks: Buffer[] = [];
        req.on('data', (chunk) => {
          bodyChunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
        });
        req.on('end', () => {
          const requestBody = Buffer.concat(bodyChunks);
          const gitProtocol = req.headers['git-protocol'];
          const env = {
            ...process.env,
            GIT_HTTP_EXPORT_ALL: '1',
            GIT_PROJECT_ROOT: repoRoot,
            PATH_INFO: url.pathname,
            QUERY_STRING: url.search.startsWith('?') ? url.search.slice(1) : url.search,
            REQUEST_METHOD: req.method ?? 'GET',
            CONTENT_TYPE: String(req.headers['content-type'] ?? ''),
            CONTENT_LENGTH: String(requestBody.length),
            REMOTE_ADDR: '127.0.0.1',
            GIT_PROTOCOL: typeof gitProtocol === 'string' ? gitProtocol : '',
            HTTP_GIT_PROTOCOL: typeof gitProtocol === 'string' ? gitProtocol : '',
          };

          const child = spawn('git', ['http-backend'], { env });
          const stdout: Buffer[] = [];
          const stderr: Buffer[] = [];
          child.stdout.on('data', (chunk) => {
            stdout.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
          });
          child.stderr.on('data', (chunk) => {
            stderr.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
          });
          child.on('error', (error) => {
            res.writeHead(500, { 'Content-Type': 'text/plain' });
            res.end(String(error));
          });
          child.on('close', (code) => {
            const output = Buffer.concat(stdout);
            const headerSep = output.indexOf(Buffer.from('\r\n\r\n'));
            const altSep = output.indexOf(Buffer.from('\n\n'));
            const sepIndex = headerSep >= 0 ? headerSep : altSep;
            const sepLen = headerSep >= 0 ? 4 : altSep >= 0 ? 2 : 0;
            if (code !== 0 && sepIndex === -1) {
              res.writeHead(500, { 'Content-Type': 'text/plain' });
              res.end(Buffer.concat(stderr));
              return;
            }
            if (sepIndex === -1) {
              res.writeHead(500, { 'Content-Type': 'text/plain' });
              res.end(output);
              return;
            }
            const headerText = output.subarray(0, sepIndex).toString('utf8');
            const responseBody = output.subarray(sepIndex + sepLen);
            let status = 200;
            const headers: Record<string, string> = {};
            for (const line of headerText.split(/\r?\n/)) {
              if (!line) continue;
              const colon = line.indexOf(':');
              if (colon === -1) continue;
              const name = line.slice(0, colon);
              const value = line.slice(colon + 1).trim();
              if (name.toLowerCase() === 'status') {
                status = Number.parseInt(value, 10) || 200;
              } else {
                headers[name] = value;
              }
            }
            res.writeHead(status, headers);
            res.end(responseBody);
          });
          child.stdin.end(requestBody);
        });
      };
    }

    async function listen(server: HttpsServer): Promise<number> {
      await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
      return (server.address() as import('node:net').AddressInfo).port;
    }

    beforeAll(async () => {
      repoRoot = mkdtempSync(join(tmpdir(), 'agentos-git-https-'));
      const worktree = join(repoRoot, 'worktree');
      const origin = join(repoRoot, 'origin.git');

      runHostGit(['-c', 'init.defaultBranch=main', 'init', worktree]);
      writeFileSync(join(worktree, 'README.md'), 'remote smart clone\n');
      runHostGit(['-C', worktree, 'add', 'README.md']);
      runHostGit([
        '-C', worktree,
        '-c', 'user.name=secure-exec', '-c', 'user.email=agent@example.com',
        'commit', '-m', 'seed',
      ]);
      runHostGit(['-C', worktree, 'checkout', '-b', 'feature/deep']);
      writeFileSync(join(worktree, 'feature.txt'), 'remote branch payload\n');
      runHostGit(['-C', worktree, 'add', 'feature.txt']);
      runHostGit([
        '-C', worktree,
        '-c', 'user.name=secure-exec', '-c', 'user.email=agent@example.com',
        'commit', '-m', 'feature branch',
      ]);
      runHostGit(['-C', worktree, 'checkout', 'main']);
      runHostGit(['clone', '--bare', worktree, origin]);
      runHostGit(['-C', origin, 'repack', '-a', '-d', '-f', '--depth=50', '--window=50']);
      // Accept anonymous pushes over smart HTTP.
      runHostGit(['-C', origin, 'config', 'http.receivepack', 'true']);

      const trusted = makeCaSignedCert('AgentOS Git Test Root CA');
      trustedCaPem = trusted.caPem;
      trustedServer = createHttpsServer(
        { key: trusted.serverKey, cert: trusted.serverCert },
        makeBackendHandler(),
      );
      trustedPort = await listen(trustedServer);

      const untrusted = makeCaSignedCert('AgentOS Git Untrusted CA');
      untrustedCaPem = untrusted.caPem;
      untrustedServer = createHttpsServer(
        { key: untrusted.serverKey, cert: untrusted.serverCert },
        makeBackendHandler(),
      );
      untrustedPort = await listen(untrustedServer);
    });

    afterAll(async () => {
      if (trustedServer) await new Promise<void>((r) => trustedServer.close(() => r()));
      if (untrustedServer) await new Promise<void>((r) => untrustedServer.close(() => r()));
      rmSync(repoRoot, { recursive: true, force: true });
    });

    const trustedUrl = () => `https://127.0.0.1:${trustedPort}/origin.git`;
    const untrustedUrl = () => `https://127.0.0.1:${untrustedPort}/origin.git`;

    it('clone fetches refs and worktree contents over HTTPS with a trusted CA', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([trustedPort], trustedCaPem));

      const res = await kernel.exec(git(`clone ${trustedUrl()} /tmp/clone`), {
        env: { GIT_CURL_VERBOSE: '1' },
      });
      expect(res.exitCode).toBe(0);
      // Proof a real TLS handshake happened in-guest (mbedTLS via libcurl).
      expect(res.stderr).toMatch(/SSL connection|TLS|SSL certificate|CAfile/i);

      const head = new TextDecoder().decode(await kernel.readFile('/tmp/clone/.git/HEAD'));
      expect(head.trim()).toBe('ref: refs/heads/main');
      const readme = new TextDecoder().decode(await kernel.readFile('/tmp/clone/README.md'));
      expect(readme).toBe('remote smart clone\n');
      await expectGitRef(kernel, '/tmp/clone', 'refs/remotes/origin/feature/deep');

      await run(kernel, git('-C /tmp/clone checkout feature/deep'));
      const feature = new TextDecoder().decode(await kernel.readFile('/tmp/clone/feature.txt'));
      expect(feature).toBe('remote branch payload\n');
    });

    it('fetch picks up a new remote branch over HTTPS', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([trustedPort], trustedCaPem));
      await run(kernel, git(`clone ${trustedUrl()} /tmp/clone`));

      // Add a new branch on the origin (host side), then fetch it in-guest.
      const bareBranch = 'fetched-branch';
      runHostGit(['-C', join(repoRoot, 'origin.git'), 'branch', bareBranch, 'main']);

      await run(kernel, git('-C /tmp/clone fetch origin'));
      await expectGitRef(kernel, '/tmp/clone', `refs/remotes/origin/${bareBranch}`);
    });

    it('push sends a small commit over HTTPS smart-HTTP', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([trustedPort], trustedCaPem));
      await run(kernel, git(`clone ${trustedUrl()} /tmp/clone`));

      await kernel.writeFile('/tmp/clone/pushed.txt', 'pushed over https\n');
      await run(kernel, git('-C /tmp/clone add pushed.txt'));
      await run(kernel, git("-C /tmp/clone commit -m 'push small'"));
      const pushed = await kernel.exec(git('-C /tmp/clone push origin HEAD:refs/heads/small-push'));
      expect(pushed.exitCode).toBe(0);

      // Verify the ref really landed in the origin bare repo (host side).
      const originRef = spawnSync(
        'git',
        ['-C', join(repoRoot, 'origin.git'), 'rev-parse', '--verify', 'refs/heads/small-push'],
        { encoding: 'utf8' },
      );
      expect(originRef.status).toBe(0);
      expect(originRef.stdout.trim()).toMatch(/^[0-9a-f]{40,64}$/);
    });

    it('push streams a >1 MiB commit over HTTPS (chunked POST)', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([trustedPort], trustedCaPem));
      await run(kernel, git(`clone ${trustedUrl()} /tmp/clone`));

      // Incompressible >1 MiB payload so the pack exceeds http.postBuffer (1 MiB)
      // and libcurl must use chunked Transfer-Encoding + Expect: 100-continue.
      const { randomBytes } = await import('node:crypto');
      const big = randomBytes(2 * 1024 * 1024);
      await kernel.writeFile('/tmp/clone/big.bin', big);
      await run(kernel, git('-C /tmp/clone add big.bin'));
      await run(kernel, git("-C /tmp/clone commit -m 'push large'"));
      const pushed = await kernel.exec(git('-C /tmp/clone push origin HEAD:refs/heads/large-push'));
      expect(pushed.exitCode).toBe(0);

      const originRef = spawnSync(
        'git',
        ['-C', join(repoRoot, 'origin.git'), 'rev-parse', '--verify', 'refs/heads/large-push'],
        { encoding: 'utf8' },
      );
      expect(originRef.status).toBe(0);
      // Confirm the large object is actually present in the origin object store.
      const cat = spawnSync(
        'git',
        ['-C', join(repoRoot, 'origin.git'), 'cat-file', '-s', `${originRef.stdout.trim()}^{tree}`],
        { encoding: 'utf8' },
      );
      expect(cat.status).toBe(0);
    });

    it('clone fails with a real certificate-verification error on an untrusted CA', async () => {
      // Only the trusted CA is seeded; the untrusted server's CA is absent.
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([untrustedPort], trustedCaPem));

      const res = await kernel.exec(git(`clone ${untrustedUrl()} /tmp/clone`));
      expect(res.exitCode).not.toBe(0);
      expect(res.stderr).toMatch(/certificate|SSL|TLS|verify|CAfile|unable to (access|get local)/i);
      expect(res.stderr).not.toContain('GitSubcommandUnsupported');
      expect(await vfs.exists('/tmp/clone/.git')).toBe(false);
    });

    it('http.sslVerify=false bypasses verification for an untrusted CA', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([untrustedPort], trustedCaPem));

      const res = await kernel.exec(git(`-c http.sslVerify=false clone ${untrustedUrl()} /tmp/clone`));
      expect(res.exitCode).toBe(0);
      const readme = new TextDecoder().decode(await kernel.readFile('/tmp/clone/README.md'));
      expect(readme).toBe('remote smart clone\n');
    });

    it('GIT_SSL_NO_VERIFY bypasses verification for an untrusted CA', async () => {
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([untrustedPort], trustedCaPem));

      const res = await kernel.exec(git(`clone ${untrustedUrl()} /tmp/clone`), {
        env: { GIT_SSL_NO_VERIFY: '1' },
      });
      expect(res.exitCode).toBe(0);
      expect(await vfs.exists('/tmp/clone/.git/HEAD')).toBe(true);
    });

    it('http.sslCAInfo trusts an explicitly supplied CA bundle', async () => {
      // Seed only the trusted CA in the default bundle; supply the untrusted
      // server's CA via a VFS file referenced with http.sslCAInfo.
      ({ kernel, vfs, dispose } = await createGitKernelWithNet([untrustedPort], trustedCaPem));
      await vfs.mkdir('/tmp/ca', { recursive: true });
      await kernel.writeFile('/tmp/ca/untrusted.pem', untrustedCaPem);

      const res = await kernel.exec(
        git(`-c http.sslCAInfo=/tmp/ca/untrusted.pem clone ${untrustedUrl()} /tmp/clone`),
      );
      expect(res.exitCode).toBe(0);
      const readme = new TextDecoder().decode(await kernel.readFile('/tmp/clone/README.md'));
      expect(readme).toBe('remote smart clone\n');
    });
  });
});
