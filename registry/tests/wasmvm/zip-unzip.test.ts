/**
 * Integration tests for zip and unzip C commands.
 *
 * Verifies zip/unzip roundtrip, recursive compression, list mode,
 * and extract-to-directory via kernel.exec() with real WASM binaries.
 */

import { describe, it, expect, afterEach } from 'vitest';
import { createInMemoryFileSystem, createWasmVmRuntime } from '@rivet-dev/agent-os-core/test/runtime';
import { C_BUILD_DIR, COMMANDS_DIR, createKernel } from '../helpers.js';
import type { Kernel } from '../helpers.js';

describe('zip/unzip commands', () => {
  let kernel: Kernel;

  afterEach(async () => {
    await kernel?.dispose();
  });

  it('zip creates valid archive, unzip extracts it, contents match', async () => {
    const vfs = createInMemoryFileSystem();
    await vfs.writeFile('/hello.txt', 'Hello, World!\n');

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    // Create zip archive
    const zipResult = await kernel.exec('zip /archive.zip /hello.txt');
    expect(zipResult.exitCode, zipResult.stderr).toBe(0);

    // Verify archive was created
    expect(await vfs.exists('/archive.zip')).toBe(true);

    // Extract to a different directory
    const unzipResult = await kernel.exec('unzip -d /extracted /archive.zip');
    expect(unzipResult.exitCode, unzipResult.stderr).toBe(0);

    // Verify extracted content matches original
    const extracted = await vfs.readTextFile('/extracted/hello.txt');
    expect(extracted).toBe('Hello, World!\n');
  });

  it('zip -r compresses directory recursively', async () => {
    const vfs = createInMemoryFileSystem();
    await vfs.mkdir('/mydir');
    await vfs.writeFile('/mydir/a.txt', 'file a\n');
    await vfs.writeFile('/mydir/b.txt', 'file b\n');

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const zipResult = await kernel.exec('zip -r /dir.zip /mydir');
    expect(zipResult.exitCode, zipResult.stderr).toBe(0);
    expect(await vfs.exists('/dir.zip')).toBe(true);

    // Extract and verify
    const unzipResult = await kernel.exec('unzip -d /out /dir.zip');
    expect(unzipResult.exitCode, unzipResult.stderr).toBe(0);

    const a = await vfs.readTextFile('/out/mydir/a.txt');
    const b = await vfs.readTextFile('/out/mydir/b.txt');
    expect(a).toBe('file a\n');
    expect(b).toBe('file b\n');
  });

  it('unzip -l lists archive contents with sizes', async () => {
    const vfs = createInMemoryFileSystem();
    await vfs.writeFile('/data.txt', 'some data content\n');

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    // Create archive first
    const zipResult = await kernel.exec('zip /list-test.zip /data.txt');
    expect(zipResult.exitCode, zipResult.stderr).toBe(0);

    // List contents
    const listResult = await kernel.exec('unzip -l /list-test.zip');
    expect(listResult.exitCode, listResult.stderr).toBe(0);
    expect(listResult.stdout).toContain('data.txt');
    // Should show the file size (18 bytes)
    expect(listResult.stdout).toContain('18');
    // Should show summary line with file count
    expect(listResult.stdout).toMatch(/1 file/);
  });

  it('zip/unzip roundtrip preserves file contents exactly', async () => {
    const vfs = createInMemoryFileSystem();
    // Binary-like content with various byte values
    const content = new Uint8Array(256);
    for (let i = 0; i < 256; i++) content[i] = i;
    await vfs.writeFile('/binary.bin', content);

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const zipResult = await kernel.exec('zip /roundtrip.zip /binary.bin');
    expect(zipResult.exitCode, zipResult.stderr).toBe(0);

    const unzipResult = await kernel.exec('unzip -d /rt-out /roundtrip.zip');
    expect(unzipResult.exitCode, unzipResult.stderr).toBe(0);

    const extracted = await vfs.readFile('/rt-out/binary.bin');
    expect(extracted.length).toBe(256);
    for (let i = 0; i < 256; i++) {
      expect(extracted[i]).toBe(i);
    }
  });

  it('unzip -d extracts to specified directory', async () => {
    const vfs = createInMemoryFileSystem();
    await vfs.writeFile('/src.txt', 'target content\n');

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const zipResult = await kernel.exec('zip /dest-test.zip /src.txt');
    expect(zipResult.exitCode, zipResult.stderr).toBe(0);

    // Extract to a new directory
    const unzipResult = await kernel.exec('unzip -d /custom-dir /dest-test.zip');
    expect(unzipResult.exitCode, unzipResult.stderr).toBe(0);

    expect(await vfs.exists('/custom-dir/src.txt')).toBe(true);
    const extracted = await vfs.readTextFile('/custom-dir/src.txt');
    expect(extracted).toBe('target content\n');
  });
});
