import { spawn, execFile, type ChildProcess } from 'node:child_process';
import { promisify } from 'node:util';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { createSocket } from 'node:dgram';
import { createKernel, createInMemoryFileSystem, createWasmVmRuntime, C_BUILD_DIR, COMMANDS_DIR, allowAll } from '@rivet-dev/agentos-test-harness';
export const nativeExec = promisify(execFile);
export const payload = Buffer.from(Array.from({ length: 128 * 1024 }, (_, i) => i % 251));
export const quote = (value: string) => "'" + value.replaceAll("'", "'\\''") + "'";
export async function unusedPort(udp: boolean): Promise<number> {
  if (udp) {
    const socket = createSocket('udp4');
    socket.bind(0, '127.0.0.1'); await once(socket, 'listening');
    const port = socket.address().port; socket.close(); await once(socket, 'close'); return port;
  }
  const server = createServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const port = (server.address() as {port: number}).port;
  await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve())); return port;
}
const spawnErrors = new WeakMap<ChildProcess, Error>();
export function startFixture(script: string, args: string[]): ChildProcess {
  const child = spawn('bash', [script, ...args], { stdio: ['ignore', 'pipe', 'pipe'], detached: true });
  child.on('error', error => spawnErrors.set(child, error));
  // Output is drained continuously; retain no unbounded diagnostic buffer.
  child.stdout!.on('data', chunk => process.stdout.write(chunk));
  child.stderr!.on('data', chunk => process.stderr.write(chunk));
  return child;
}
export async function stopFixture(child: ChildProcess | undefined) {
  if (!child || !child.pid || child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, 'exit');
  const signal = (value: NodeJS.Signals) => {
    try { process.kill(-child.pid!, value); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ESRCH') throw error; }
  };
  signal('SIGTERM'); const timer = setTimeout(() => signal('SIGKILL'), 2000);
  try { await exited; } finally { clearTimeout(timer); }
}
export async function ready(child: ChildProcess, probe: () => Promise<void>) {
  let last: unknown;
  for (let attempt = 0; attempt < 20; attempt++) {
    if (spawnErrors.has(child)) throw spawnErrors.get(child);
    if (child.exitCode !== null || child.signalCode !== null) throw new Error(`fixture exited: ${child.exitCode}/${child.signalCode}`);
    try { await probe(); return; } catch (error) { last = error; }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error('fixture readiness failed after 20 bounded probes', { cause: last });
}
export async function withVm(ports: number[], files: Record<string, Uint8Array>, deny: boolean, run: (kernel: ReturnType<typeof createKernel>) => Promise<void>) {
  const filesystem = createInMemoryFileSystem();
  const kernel = createKernel({filesystem, permissions: deny ? {network: 'deny'} : allowAll, loopbackExemptPorts: deny ? [] : ports});
  try {
    await kernel.mount(createWasmVmRuntime({commandDirs: [C_BUILD_DIR, COMMANDS_DIR]}));
    await filesystem.mkdir('/tmp', {recursive: true});
    for (const [name, data] of Object.entries(files)) await kernel.writeFile(`/tmp/${name}`, data);
    await run(kernel);
  } finally { await kernel.dispose(); }
}
