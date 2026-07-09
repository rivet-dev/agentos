// POC stage-1 assertions: node's REAL lib/path.js and lib/buffer.js, loaded by
// bootstrap.js, must behave like node. Throws one aggregate error on failure.
(function () {
  'use strict';
  const { requireBuiltin } = globalThis.__poc;
  const failures = [];

  function check(name, actual, expected) {
    const a = JSON.stringify(actual);
    const e = JSON.stringify(expected);
    if (a !== e) failures.push(`${name}: got ${a}, want ${e}`);
  }

  // ---------- path ----------
  const path = requireBuiltin('path');
  check('path.join', path.join('/a', 'b', '..', 'c'), '/a/c');
  check('path.join dots', path.join('.', 'x/y', './z'), 'x/y/z');
  check('path.resolve', path.resolve('/foo/bar', './baz'), '/foo/bar/baz');
  check('path.resolve up', path.resolve('/foo/bar', '../qux'), '/foo/qux');
  check('path.normalize', path.normalize('/foo/bar//baz/asdf/quux/..'), '/foo/bar/baz/asdf');
  check('path.dirname', path.dirname('/foo/bar/baz.txt'), '/foo/bar');
  check('path.basename', path.basename('/foo/bar/baz.txt'), 'baz.txt');
  check('path.basename ext', path.basename('/foo/bar/baz.txt', '.txt'), 'baz');
  check('path.extname', path.extname('index.coffee.md'), '.md');
  check('path.extname none', path.extname('Makefile'), '');
  check('path.relative', path.relative('/data/orandea/test/aaa', '/data/orandea/impl/bbb'), '../../impl/bbb');
  check('path.isAbsolute', [path.isAbsolute('/x'), path.isAbsolute('x')], [true, false]);
  check('path.parse', path.parse('/home/user/dir/file.txt'),
        { root: '/', dir: '/home/user/dir', base: 'file.txt', ext: '.txt', name: 'file' });
  check('path.format', path.format({ dir: '/a/b', name: 'c', ext: '.d' }), '/a/b/c.d');
  check('path.sep', path.sep, '/');
  check('path.win32.join', path.win32.join('C:\\a', 'b', '..', 'c'), 'C:\\a\\c');
  check('path.win32.basename', path.win32.basename('C:\\temp\\myfile.html'), 'myfile.html');
  check('path.win32.resolve', path.win32.resolve('C:\\foo', 'bar'), 'C:\\foo\\bar');
  check('path.posix identity', path.posix.join('a', 'b'), 'a/b');

  // ---------- buffer ----------
  const { Buffer, isUtf8, isAscii, btoa, atob } = requireBuiltin('buffer');

  check('Buffer.from utf8 roundtrip', Buffer.from('hello wörld ✓', 'utf8').toString('utf8'), 'hello wörld ✓');
  check('Buffer.from utf8 bytes', Array.from(Buffer.from('é', 'utf8')), [0xc3, 0xa9]);
  check('byteLength utf8', Buffer.byteLength('hello wörld ✓', 'utf8'), 16);
  check('byteLength astral', Buffer.byteLength('💩', 'utf8'), 4);
  check('toString hex', Buffer.from([0xde, 0xad, 0xbe, 0xef]).toString('hex'), 'deadbeef');
  check('from hex', Array.from(Buffer.from('cafebabe', 'hex')), [0xca, 0xfe, 0xba, 0xbe]);
  check('toString base64', Buffer.from('Many hands make light work.').toString('base64'),
        'TWFueSBoYW5kcyBtYWtlIGxpZ2h0IHdvcmsu');
  check('from base64', Buffer.from('TWFueSBoYW5kcyBtYWtlIGxpZ2h0IHdvcmsu', 'base64').toString('utf8'),
        'Many hands make light work.');
  check('base64url value', Buffer.from([0xfb, 0xff]).toString('base64url'), '-_8');
  check('toString latin1', Buffer.from([0x68, 0xe9]).toString('latin1'), 'hé');
  check('toString utf16le', Buffer.from([0x68, 0x00, 0x69, 0x00]).toString('utf16le'), 'hi');
  check('alloc fill num', Array.from(Buffer.alloc(3, 7)), [7, 7, 7]);
  check('alloc fill str', Buffer.alloc(6, 'ab').toString('utf8'), 'ababab');
  check('alloc zero', Array.from(Buffer.alloc(4)), [0, 0, 0, 0]);
  check('concat', Buffer.concat([Buffer.from('ab'), Buffer.from('cd')]).toString(), 'abcd');
  check('concat totalLength', Buffer.concat([Buffer.from('ab'), Buffer.from('cd')], 3).toString(), 'abc');
  check('compare eq', Buffer.compare(Buffer.from('abc'), Buffer.from('abc')), 0);
  check('compare lt', Buffer.compare(Buffer.from('abb'), Buffer.from('abc')), -1);
  check('compare len', Buffer.compare(Buffer.from('ab'), Buffer.from('abc')), -1);
  check('equals', Buffer.from('x').equals(Buffer.from('x')), true);
  check('indexOf str', Buffer.from('hello hello').indexOf('llo'), 2);
  check('indexOf str from', Buffer.from('hello hello').indexOf('llo', 3), 8);
  check('lastIndexOf str', Buffer.from('hello hello').lastIndexOf('llo'), 8);
  check('indexOf num', Buffer.from([1, 2, 3, 2]).indexOf(2), 1);
  check('indexOf buf', Buffer.from('abcdef').indexOf(Buffer.from('cd')), 2);
  check('indexOf missing', Buffer.from('abc').indexOf('zz'), -1);
  check('includes', Buffer.from('abc').includes('b'), true);
  check('slice/subarray', Buffer.from('buffer').subarray(1, 4).toString(), 'uff');
  check('subarray shares memory', (() => {
    const b = Buffer.from('abcd');
    b.subarray(1, 3)[0] = 0x7a;
    return b.toString();
  })(), 'azcd');
  check('write returns count', Buffer.alloc(4).write('abcdef'), 4);
  check('write partial multibyte', (() => {
    const b = Buffer.alloc(3);
    const n = b.write('aé é', 0, 3, 'utf8'); // 'a'(1) + 'é'(2) fits, next ' ' doesn't
    return [n, b.toString('utf8', 0, n)];
  })(), [3, 'aé']);
  check('readUInt32LE', (() => {
    const b = Buffer.alloc(4);
    b.writeUInt32LE(0xdeadbeef, 0);
    return [b.readUInt32LE(0), Array.from(b)];
  })(), [0xdeadbeef, [0xef, 0xbe, 0xad, 0xde]]);
  check('readUInt32BE', (() => {
    const b = Buffer.alloc(4);
    b.writeUInt32BE(0x01020304, 0);
    return [b.readUInt32BE(0), Array.from(b)];
  })(), [0x01020304, [1, 2, 3, 4]]);
  check('readBigUInt64LE', (() => {
    const b = Buffer.alloc(8);
    b.writeBigUInt64LE(0x0102030405060708n, 0);
    return b.readBigUInt64LE(0) === 0x0102030405060708n;
  })(), true);
  check('readDoubleLE', (() => {
    const b = Buffer.alloc(8);
    b.writeDoubleLE(3.5, 0);
    return b.readDoubleLE(0);
  })(), 3.5);
  check('swap16', Array.from(Buffer.from([1, 2, 3, 4]).swap16()), [2, 1, 4, 3]);
  check('swap32', Array.from(Buffer.from([1, 2, 3, 4]).swap32()), [4, 3, 2, 1]);
  check('isBuffer', [Buffer.isBuffer(Buffer.alloc(1)), Buffer.isBuffer(new Uint8Array(1))], [true, false]);
  check('from arraybuffer view', (() => {
    const ab = new ArrayBuffer(4);
    new Uint8Array(ab).set([9, 8, 7, 6]);
    return Array.from(Buffer.from(ab, 1, 2));
  })(), [8, 7]);
  check('from array', Array.from(Buffer.from([256 + 5, 2])), [5, 2]);
  check('isUtf8/isAscii', [
    isUtf8(Buffer.from('héllo')), isUtf8(Buffer.from([0xff, 0xfe])),
    isAscii(Buffer.from('abc')), isAscii(Buffer.from('é')),
  ], [true, false, true, false]);
  check('btoa/atob', [btoa('hello'), atob('aGVsbG8=')], ['aGVsbG8=', 'hello']);
  check('toJSON', Buffer.from([1, 2]).toJSON(), { type: 'Buffer', data: [1, 2] });
  check('fill on buffer', Array.from(Buffer.from([0, 0, 0, 0, 0]).fill('ab', 1, 4)), [0, 97, 98, 97, 0]);
  check('copy', (() => {
    const src = Buffer.from('abcdef');
    const dst = Buffer.alloc(4, 0x2e);
    const n = src.copy(dst, 1, 2, 4);
    return [n, dst.toString()];
  })(), [2, '.cd.']);

  // ---------- stage 2.5: simdutf-backed codecs (wasm compiled with our libc) ----------
  // Fixed expectations hold in both modes (wasm-backed and JS fallback).
  check('isUtf8 surrogate-encoded', isUtf8(Buffer.from([0xed, 0xa0, 0x80])), false);
  check('isUtf8 overlong', isUtf8(Buffer.from([0xc0, 0xaf])), false);
  check('isUtf8 4byte max', isUtf8(Buffer.from([0xf4, 0x8f, 0xbf, 0xbf])), true);
  check('isUtf8 beyond U+10FFFF', isUtf8(Buffer.from([0xf4, 0x90, 0x80, 0x80])), false);
  check('isUtf8 truncated', isUtf8(Buffer.from([0x61, 0xc3])), false);
  check('isUtf8/isAscii empty', [isUtf8(Buffer.alloc(0)), isAscii(Buffer.alloc(0))], [true, true]);
  check('byteLength 2byte boundary', Buffer.byteLength('߿', 'utf8'), 4);
  check('byteLength 3byte boundary', Buffer.byteLength('ࠀ￿', 'utf8'), 6);
  check('byteLength astral pair', Buffer.byteLength('💩', 'utf8'), 4);
  check('byteLength lone surrogate', Buffer.byteLength('a\ud800b', 'utf8'), 5); // U+FFFD replacement
  check('byteLength 1MiB ascii', Buffer.byteLength('a'.repeat(1 << 20), 'utf8'), 1 << 20);

  const simdutfBacked = globalThis.__pocSimdutfBacked === true;
  if (simdutfBacked) {
    // Differential check: the wasm-backed binding must agree with the JS
    // reference implementations across tricky inputs.
    const { isUtf8: jsIsUtf8, isAscii: jsIsAscii, byteLengthUtf8: jsByteLen } =
      globalThis.__pocSimdutfJsFallback;
    const trickyBuffers = [
      Buffer.alloc(0),
      Buffer.from('plain ascii'),
      Buffer.from('héllo wörld ✓ 💩'),
      Buffer.from([0xff, 0xfe, 0x80]),
      Buffer.from([0x61, 0xc3]),
      Buffer.from([0xed, 0xa0, 0x80]),
      Buffer.from([0xf4, 0x90, 0x80, 0x80]),
      Buffer.alloc(1 << 20, 0x61),
      (() => { const b = Buffer.alloc(1 << 20, 0x61); b[(1 << 20) - 1] = 0xff; return b; })(),
    ];
    trickyBuffers.forEach((b, i) => {
      check(`simdutf diff isUtf8[${i}]`, isUtf8(b), jsIsUtf8(b));
      check(`simdutf diff isAscii[${i}]`, isAscii(b), jsIsAscii(b));
    });
    const trickyStrings = [
      '', 'ascii only', 'héllo', '💩💩💩', 'mixed é 💩 text',
      '߿ࠀ￿', 'a'.repeat(100000) + '💩',
    ];
    trickyStrings.forEach((s, i) => {
      check(`simdutf diff byteLength[${i}]`, Buffer.byteLength(s, 'utf8'), jsByteLen(s));
    });
  }

  // ---------- fs (stage 2: sync ops via internalBinding('fs') shim) ----------
  const fs = requireBuiltin('fs');

  function checkThrows(name, fn, expectations) {
    try {
      fn();
      failures.push(`${name}: expected throw, got none`);
    } catch (err) {
      for (const key of Object.keys(expectations)) {
        if (err[key] !== expectations[key]) {
          failures.push(`${name}: err.${key} got ${JSON.stringify(err[key])}, want ${JSON.stringify(expectations[key])}`);
        }
      }
    }
  }

  // Some VM base layers may lack /tmp; the mem backend pre-creates it.
  try { fs.mkdirSync('/tmp'); } catch (err) { if (err.code !== 'EEXIST') throw err; }

  // utf8 write/read roundtrip (fast paths: writeFileUtf8 / readFileUtf8)
  fs.writeFileSync('/tmp/poc.txt', 'hello wörld ✓\n');
  check('fs utf8 roundtrip', fs.readFileSync('/tmp/poc.txt', 'utf8'), 'hello wörld ✓\n');

  // binary roundtrip (open/write/read/close path, no fast path)
  const payload = Buffer.from([0, 1, 2, 250, 251, 252, 253, 254, 255]);
  fs.writeFileSync('/tmp/poc.bin', payload);
  const readBack = fs.readFileSync('/tmp/poc.bin');
  check('fs binary roundtrip', [Buffer.isBuffer(readBack), Array.from(readBack)],
        [true, Array.from(payload)]);

  // appending via flag
  fs.writeFileSync('/tmp/poc.txt', 'more', { flag: 'a' });
  check('fs append flag', fs.readFileSync('/tmp/poc.txt', 'utf8'), 'hello wörld ✓\nmore');

  // statSync fields
  const st = fs.statSync('/tmp/poc.bin');
  check('stat size', st.size, 9);
  check('stat isFile/isDirectory', [st.isFile(), st.isDirectory()], [true, false]);
  check('stat dir', (() => { const d = fs.statSync('/tmp'); return [d.isDirectory(), d.isFile()]; })(),
        [true, false]);
  check('stat mode type bits', (st.mode & 0o170000) === 0o100000, true);
  check('stat mtime plausible', st.mtime instanceof Date && st.mtimeMs > 0, true);
  check('statSync throwIfNoEntry:false', fs.statSync('/tmp/nope', { throwIfNoEntry: false }), undefined);
  check('existsSync', [fs.existsSync('/tmp/poc.bin'), fs.existsSync('/tmp/nope')], [true, false]);

  // ENOENT error shape (code + errno + syscall — exercises error translation)
  checkThrows('readFileSync ENOENT', () => fs.readFileSync('/tmp/missing.txt', 'utf8'),
              { code: 'ENOENT', errno: -2, syscall: 'open' });
  checkThrows('statSync ENOENT', () => fs.statSync('/tmp/missing.txt'),
              { code: 'ENOENT', errno: -2, syscall: 'stat' });

  // mkdirSync + readdirSync
  fs.mkdirSync('/tmp/dir-a');
  fs.mkdirSync('/tmp/dir-a/nested/deep', { recursive: true });
  fs.writeFileSync('/tmp/dir-a/file1.txt', 'x');
  fs.writeFileSync('/tmp/dir-a/file2.txt', 'y');
  check('readdirSync', fs.readdirSync('/tmp/dir-a').sort(), ['file1.txt', 'file2.txt', 'nested']);
  checkThrows('mkdirSync EEXIST', () => fs.mkdirSync('/tmp/dir-a'),
              { code: 'EEXIST', syscall: 'mkdir' });
  checkThrows('readdirSync ENOENT', () => fs.readdirSync('/tmp/no-dir'),
              { code: 'ENOENT', syscall: 'scandir' });

  // unlinkSync then ENOENT
  fs.unlinkSync('/tmp/poc.bin');
  check('unlink removes', fs.existsSync('/tmp/poc.bin'), false);
  checkThrows('unlinkSync ENOENT', () => fs.unlinkSync('/tmp/poc.bin'),
              { code: 'ENOENT', errno: -2, syscall: 'unlink' });

  // openSync/readSync positional
  const fd = fs.openSync('/tmp/poc.txt', 'r');
  const buf4 = Buffer.alloc(4);
  const nRead = fs.readSync(fd, buf4, 0, 4, 6);
  fs.closeSync(fd);
  // bytes 6..10 of 'hello wörld' are w + ö(2 bytes) + r
  check('readSync positional', [nRead, buf4.toString('utf8')], [4, 'wör']);

  if (failures.length > 0) {
    throw new Error(`node-stdlib POC sync failures (${failures.length}):\n  ${failures.join('\n  ')}`);
  }

  // ---------- stage 3: ASYNC fs (callbacks, promises, ordering, streams) ----
  const { process } = globalThis.__poc;
  const timers = requireBuiltin('timers');
  const fsp = requireBuiltin('fs/promises');

  // promisify helper for callback-API assertions
  const p = (fn, ...args) => new Promise((resolve, reject) => {
    fn(...args, (err, value) => (err ? reject(err) : resolve(value)));
  });

  globalThis.__pocAsync = (async () => {
    // --- callback API roundtrips ---
    await p(fs.writeFile, '/tmp/async.txt', 'async wörld ✓');
    check('cb readFile utf8', await p(fs.readFile, '/tmp/async.txt', 'utf8'), 'async wörld ✓');
    const cbBinary = await p(fs.readFile, '/tmp/async.txt'); // no encoding: open/fstat/read/close chain
    check('cb readFile binary', [Buffer.isBuffer(cbBinary), cbBinary.toString('utf8')],
          [true, 'async wörld ✓']);
    const cbStat = await p(fs.stat, '/tmp/async.txt');
    check('cb stat', [cbStat.isFile(), cbStat.size > 0], [true, true]);
    await p(fs.mkdir, '/tmp/async-dir');
    await p(fs.writeFile, '/tmp/async-dir/one.txt', '1');
    check('cb readdir', await p(fs.readdir, '/tmp/async-dir'), ['one.txt']);
    await p(fs.unlink, '/tmp/async-dir/one.txt');
    check('cb unlink', fs.existsSync('/tmp/async-dir/one.txt'), false);

    // --- async ENOENT error shape ---
    const cbErr = await p(fs.readFile, '/tmp/no-such-async.txt', 'utf8').then(
      () => null, (err) => err);
    check('cb ENOENT fields', [cbErr?.code, cbErr?.errno, cbErr?.syscall],
          ['ENOENT', -2, 'open']);

    // --- concurrency: 10 in flight, all complete, no corruption ---
    await Promise.all(Array.from({ length: 10 }, (_, i) =>
      p(fs.writeFile, `/tmp/conc-${i}.txt`, `payload-${i}-${'x'.repeat(i * 100)}`)));
    const concReads = await Promise.all(Array.from({ length: 10 }, (_, i) =>
      p(fs.readFile, `/tmp/conc-${i}.txt`, 'utf8')));
    check('concurrent integrity',
          concReads.every((s, i) => s === `payload-${i}-${'x'.repeat(i * 100)}`), true);

    // --- fs/promises ---
    await fsp.writeFile('/tmp/promises.txt', 'via promises');
    check('fsp readFile', await fsp.readFile('/tmp/promises.txt', 'utf8'), 'via promises');
    const pStat = await fsp.stat('/tmp/promises.txt');
    check('fsp stat', [pStat.isFile(), pStat.size], [true, 12]);
    await fsp.mkdir('/tmp/promises-dir');
    check('fsp mkdir', fs.statSync('/tmp/promises-dir').isDirectory(), true);
    const pErr = await fsp.readFile('/tmp/nope-promises.txt', 'utf8').then(() => null, (e) => e);
    check('fsp ENOENT rejection', [pErr?.code, pErr?.errno], ['ENOENT', -2]);

    // --- ordering probes ---
    const order1 = [];
    const fsDone = new Promise((resolve) => {
      fs.stat('/tmp', () => { order1.push('fs'); resolve(); });
    });
    process.nextTick(() => order1.push('tick'));
    Promise.resolve().then(() => order1.push('micro'));
    order1.push('sync');
    await fsDone;
    check('ordering: sync first', order1[0], 'sync');
    check('ordering: fs completion last', order1[order1.length - 1], 'fs');
    check('ordering: tick before fs', order1.indexOf('tick') < order1.indexOf('fs'), true);
    // node also guarantees micro before fs; record observed order (see notes)
    globalThis.__pocOrder1 = order1.join(',');

    const order2 = [];
    await Promise.all([
      new Promise((resolve) => fs.stat('/tmp', () => {
        order2.push('cb1');
        process.nextTick(() => order2.push('tick-in-cb1'));
        resolve();
      })),
      new Promise((resolve) => fs.stat('/tmp', () => { order2.push('cb2'); resolve(); })),
    ]);
    check('ordering: nextTick in completion before next completion',
          order2.join(','), 'cb1,tick-in-cb1,cb2');

    const order3 = [];
    await Promise.all([
      new Promise((resolve) => timers.setImmediate(() => { order3.push('immediate'); resolve(); })),
      new Promise((resolve) => fs.stat('/tmp', () => { order3.push('fs'); resolve(); })),
    ]);
    check('ordering: immediate and fs completion both ran', order3.length, 2);
    globalThis.__pocOrder3 = order3.join(','); // observed order recorded, not asserted

    // --- setTimeout sanity through real lib/timers.js ---
    check('setTimeout fires', await new Promise((resolve) =>
      timers.setTimeout(() => resolve('timer'), 5)), 'timer');

    // --- streams probe: createReadStream / createWriteStream / pipe ---
    const MB = 1024 * 1024;
    const big = Buffer.alloc(MB);
    for (let i = 0; i < MB; i++) big[i] = i % 251;
    fs.writeFileSync('/tmp/big.bin', big);

    const chunks = [];
    const events = [];
    await new Promise((resolve, reject) => {
      const rs = fs.createReadStream('/tmp/big.bin');
      rs.on('data', (chunk) => chunks.push(chunk));
      rs.on('end', () => events.push('end'));
      rs.on('close', () => { events.push('close'); resolve(); });
      rs.on('error', reject);
    });
    const collected = Buffer.concat(chunks);
    check('stream read length', collected.length, MB);
    check('stream read content', collected.equals(big), true);
    check('stream read got multiple chunks', chunks.length > 1, true);
    check('stream read events', events, ['end', 'close']);

    await new Promise((resolve, reject) => {
      const ws = fs.createWriteStream('/tmp/ws.bin');
      ws.on('error', reject);
      ws.on('close', resolve);
      ws.write(big.subarray(0, 512 * 1024));
      ws.end(big.subarray(512 * 1024));
    });
    check('write stream content', fs.readFileSync('/tmp/ws.bin').equals(big), true);

    await new Promise((resolve, reject) => {
      const rs = fs.createReadStream('/tmp/big.bin');
      const ws = fs.createWriteStream('/tmp/copy.bin');
      ws.on('close', resolve);
      rs.on('error', reject);
      ws.on('error', reject);
      rs.pipe(ws);
    });
    check('pipe copy', fs.readFileSync('/tmp/copy.bin').equals(big), true);

    if (failures.length > 0) {
      throw new Error(`node-stdlib POC async failures (${failures.length}):\n  ${failures.join('\n  ')}`);
    }
    return `ok: all sync+async checks passed (${simdutfBacked ? 'simdutf wasm-backed' : 'js codecs'})`;
  })();

  globalThis.__pocAsync.then(
    (result) => {
      globalThis.__pocResult = result;
      // Guest-probe harnesses read a single JSON line from stdout, printed only
      // once async work has genuinely finished (doubles as a liveness proof).
      if (typeof console !== 'undefined' && typeof console.log === 'function') {
        console.log(JSON.stringify({
          poc: result,
          order1: globalThis.__pocOrder1,
          order3: globalThis.__pocOrder3,
        }));
      }
    },
    (err) => {
      globalThis.__pocResult = `FAILED: ${err?.stack ?? err}`;
      if (typeof console !== 'undefined' && typeof console.error === 'function') {
        console.error(globalThis.__pocResult);
      }
    },
  );
})();
