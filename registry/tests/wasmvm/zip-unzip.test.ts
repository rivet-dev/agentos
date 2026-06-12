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

interface HostileEntry {
  name: string;
  method: number;            // 0 = store, 8 = deflate
  compressedSize: number;
  uncompressedSize: number;
  localOffset: number;
}

/** Builds a ZIP whose EOCD cd-size field is corrupt so minizip rejects it and
 *  unzip's raw central-directory fallback parser is exercised. The nonzero
 *  version fields on each central directory record also make minizip reject
 *  the archive under the VM's stream semantics, where its reopen-based seek
 *  callback reads EOCD fields from offset 0 instead of the EOCD record.
 *  `prefix` bytes (e.g. a real local file header) are placed at offset 0. */
function buildFallbackArchive(prefix: Uint8Array, entries: HostileEntry[]): Uint8Array {
  const enc = new TextEncoder();
  const cdParts: Uint8Array[] = [];
  for (const e of entries) {
    const nameBytes = enc.encode(e.name);
    const cd = new Uint8Array(46 + nameBytes.length);
    const dv = new DataView(cd.buffer);
    dv.setUint32(0, 0x02014b50, true);   // central directory signature
    dv.setUint16(4, 20, true);           // version made by
    dv.setUint16(6, 20, true);           // version needed to extract
    dv.setUint16(10, e.method, true);
    dv.setUint32(20, e.compressedSize, true);
    dv.setUint32(24, e.uncompressedSize, true);
    dv.setUint16(28, nameBytes.length, true);
    dv.setUint32(42, e.localOffset, true);
    cd.set(nameBytes, 46);
    cdParts.push(cd);
  }
  const cdOffset = prefix.length;
  const cdLen = cdParts.reduce((n, p) => n + p.length, 0);
  const eocd = new Uint8Array(22);
  const dv = new DataView(eocd.buffer);
  dv.setUint32(0, 0x06054b50, true);     // EOCD signature
  dv.setUint16(8, entries.length, true); // entries on this disk
  dv.setUint16(10, entries.length, true);// total entries
  dv.setUint32(12, 0xffffffff, true);    // corrupt cd size: forces the fallback parser
  dv.setUint32(16, cdOffset, true);
  const out = new Uint8Array(prefix.length + cdLen + 22);
  out.set(prefix, 0);
  let off = cdOffset;
  for (const p of cdParts) { out.set(p, off); off += p.length; }
  out.set(eocd, off);
  return out;
}

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

  it('fallback parser rejects an entry with a wrapping local offset', async () => {
    const vfs = createInMemoryFileSystem();
    const bytes = buildFallbackArchive(new Uint8Array(0), [
      { name: 'evil.txt', method: 0, compressedSize: 4, uncompressedSize: 4, localOffset: 0xfffffff0 },
    ]);
    await vfs.writeFile('/evil.zip', bytes);

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const result = await kernel.exec('unzip -d /out /evil.zip');
    expect(result.exitCode, result.stderr).toBe(1);
    expect(result.stderr).toMatch(/error/);
    expect(await vfs.exists('/out/evil.txt')).toBe(false);
  });

  it('fallback parser skips an entry whose normalized name is empty', async () => {
    const vfs = createInMemoryFileSystem();
    const bytes = buildFallbackArchive(new Uint8Array(0), [
      { name: '/', method: 0, compressedSize: 0, uncompressedSize: 0, localOffset: 0 },
    ]);
    await vfs.writeFile('/empty-name.zip', bytes);

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const result = await kernel.exec('unzip /empty-name.zip');
    expect(result.exitCode, result.stderr).toBe(0);
    expect(result.stdout).not.toMatch(/error/);
    expect(result.stderr).not.toMatch(/error/);
  });

  it('fallback parser caps hostile uncompressed sizes before allocating', async () => {
    const vfs = createInMemoryFileSystem();
    // A real 31-byte local header for a 1-byte stored payload.
    const prefix = new Uint8Array(31);
    const pdv = new DataView(prefix.buffer);
    pdv.setUint32(0, 0x04034b50, true); // local file header signature
    pdv.setUint16(4, 20, true);         // version needed to extract
    pdv.setUint16(26, 0, true);         // name length
    pdv.setUint16(28, 0, true);         // extra length
    prefix[30] = 0x41;                  // one payload byte
    const bytes = buildFallbackArchive(prefix, [
      { name: 'big.bin', method: 0, compressedSize: 1, uncompressedSize: 0xffffffff, localOffset: 0 },
    ]);
    await vfs.writeFile('/big.zip', bytes);

    kernel = createKernel({ filesystem: vfs });
    await kernel.mount(
      createWasmVmRuntime({ commandDirs: [C_BUILD_DIR, COMMANDS_DIR] }),
    );

    const result = await kernel.exec('unzip -d /cap-out /big.zip');
    expect(result.exitCode, result.stderr).toBe(1);
    expect(result.stderr).toMatch(/too large/);
    expect(await vfs.exists('/cap-out/big.bin')).toBe(false);
  });
});
