import { describe, it, expect, afterEach } from 'vitest';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { createWasmVmRuntime } from '@rivet-dev/agent-os-posix';
import { createKernel } from '@secure-exec/core';
import { COMMANDS_DIR, hasWasmBinaries } from '../helpers.js';
import type { Kernel } from '@secure-exec/core';

const CODEDB_DIR = resolve(process.cwd(), 'software/codedb/wasm');
const hasCodedbBinary = existsSync(resolve(CODEDB_DIR, 'codedb'));

class SimpleVFS {
  private files = new Map<string, Uint8Array>();
  private dirs = new Set<string>(['/']);

  async readFile(path: string): Promise<Uint8Array> {
    const data = this.files.get(path);
    if (!data) throw new Error(`ENOENT: ${path}`);
    return data;
  }

  async readTextFile(path: string): Promise<string> {
    return new TextDecoder().decode(await this.readFile(path));
  }

  async readDir(path: string): Promise<string[]> {
    const prefix = path === '/' ? '/' : path + '/';
    const entries: string[] = [];
    for (const p of [...this.files.keys(), ...this.dirs]) {
      if (p !== path && p.startsWith(prefix)) {
        const rest = p.slice(prefix.length);
        if (!rest.includes('/')) entries.push(rest);
      }
    }
    return entries;
  }

  async readDirWithTypes(path: string) {
    return (await this.readDir(path)).map(name => ({
      name,
      isDirectory: this.dirs.has(path === '/' ? `/${name}` : `${path}/${name}`),
    }));
  }

  async writeFile(path: string, content: string | Uint8Array): Promise<void> {
    const data = typeof content === 'string' ? new TextEncoder().encode(content) : content;
    this.files.set(path, new Uint8Array(data));
    const parts = path.split('/').filter(Boolean);
    for (let i = 1; i < parts.length; i++) {
      this.dirs.add('/' + parts.slice(0, i).join('/'));
    }
  }

  async mkdir(path: string, _options?: { recursive?: boolean }) {
    this.dirs.add(path);
    const parts = path.split('/').filter(Boolean);
    for (let i = 1; i < parts.length; i++) {
      this.dirs.add('/' + parts.slice(0, i).join('/'));
    }
  }

  async exists(path: string): Promise<boolean> {
    return this.files.has(path) || this.dirs.has(path);
  }

  async stat(path: string) {
    const isDir = this.dirs.has(path);
    const data = this.files.get(path);
    if (!isDir && !data) throw new Error(`ENOENT: ${path}`);
    return {
      mode: isDir ? 0o40755 : 0o100644,
      size: data?.length ?? 0,
      isDirectory: isDir,
      isSymbolicLink: false,
      atimeMs: Date.now(),
      mtimeMs: Date.now(),
      ctimeMs: Date.now(),
      birthtimeMs: Date.now(),
      ino: 0,
      nlink: 1,
      uid: 1000,
      gid: 1000,
    };
  }

  async lstat(path: string) { return this.stat(path); }
  async removeFile(path: string) { this.files.delete(path); }
  async removeDir(path: string) { this.dirs.delete(path); }

  async rename(oldPath: string, newPath: string) {
    const data = this.files.get(oldPath);
    if (data) {
      this.files.set(newPath, data);
      this.files.delete(oldPath);
    }
  }

  async pread(path: string, buffer: Uint8Array, offset: number, length: number, position: number): Promise<number> {
    const data = this.files.get(path);
    if (!data) throw new Error(`ENOENT: ${path}`);
    const available = Math.min(length, data.length - position);
    if (available <= 0) return 0;
    buffer.set(data.subarray(position, position + available), offset);
    return available;
  }
}

async function createTestKernel(): Promise<{ kernel: Kernel; vfs: SimpleVFS }> {
  const vfs = new SimpleVFS();
  const kernel = createKernel({ filesystem: vfs as any });
  await kernel.mount(createWasmVmRuntime({ commandDirs: [CODEDB_DIR, COMMANDS_DIR] }));
  return { kernel, vfs };
}

describe.skipIf(!hasWasmBinaries || !hasCodedbBinary)('codedb reduced WASI fork', () => {
  let kernel: Kernel;

  afterEach(async () => {
    await kernel?.dispose();
  });

  it('indexes a small project and answers tree, outline, search, deps, and read queries', async () => {
    let vfs: SimpleVFS;
    ({ kernel, vfs } = await createTestKernel());

    await vfs.writeFile('/proj/src/main.zig', 'const util = @import("util.zig");\npub fn main() void {\n    _ = util.value;\n}\n');
    await vfs.writeFile('/proj/src/util.zig', 'pub const value = 42;\npub fn helper() void {}\n');
    await vfs.writeFile('/proj/src/util.ts', 'export function AgentOs() {\n  return 42;\n}\n');
    await vfs.writeFile('/proj/src/consumer.ts', 'import { AgentOs } from "./util.ts";\nexport const value = AgentOs();\n');
    await vfs.writeFile('/proj/src/mod.py', 'import os\n\ndef handle_auth():\n    return os.getcwd()\n');

    const tree = await kernel.exec('codedb tree /proj');
    expect(tree.stdout).toContain('src/');
    expect(tree.stdout).toContain('main.zig  zig');
    expect(tree.stdout).toContain('util.ts  typescript');

    const outline = await kernel.exec('codedb outline /proj src/util.ts');
    expect(outline.stdout).toContain('src/util.ts typescript');
    expect(outline.stdout).toContain('function AgentOs');

    const search = await kernel.exec('codedb search /proj AgentOs 5');
    expect(search.stdout).toContain('src/util.ts:1');
    expect(search.stdout).toContain('src/consumer.ts:1');

    const deps = await kernel.exec('codedb deps /proj src/util.ts');
    expect(deps.stdout).toContain('src/consumer.ts');

    const read = await kernel.exec('codedb read /proj src/mod.py');
    expect(read.stdout).toContain('def handle_auth():');
  });
});
