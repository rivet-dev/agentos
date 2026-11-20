// Finite differential programs. Keep original RED/Node reference programs unchanged.

export const timingSafeViewMetadataProbe = String.raw`import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const cjs = require('node:crypto');
const esm = await import('node:crypto');
const rows = [];
const shapeNames = ['Uint8Array', 'Buffer', 'DataView', 'SharedArrayBufferView', 'ArrayBuffer', 'SharedArrayBuffer'];
function make(shape, byte) {
  const shared = shape.startsWith('Shared');
  const backing = shared ? new SharedArrayBuffer(3) : new ArrayBuffer(3);
  new Uint8Array(backing).set([7, byte, 7]);
  const value = shape === 'Buffer' ? Buffer.from(backing, 1, 1)
    : shape === 'DataView' ? new DataView(backing, 1, 1)
    : shape.endsWith('ArrayBuffer') ? backing : new Uint8Array(backing, 1, 1);
  return { value, backing };
}
for (const [route, crypto] of [['CJS', cjs], ['ESM', esm]]) {
  for (const shape of shapeNames) {
    for (const property of ['buffer', 'byteOffset', 'byteLength']) {
      for (const mode of ['own-value', 'own-getter']) {
        const left = make(shape, 1), right = make(shape, 2);
        let reads = 0;
        for (const item of [left, right]) {
          const replacement = property === 'buffer' ? new Uint8Array([7, 7, 7]).buffer : 0;
          Object.defineProperty(item.value, property, mode === 'own-value'
            ? { value: replacement, configurable: true }
            : { get() { reads++; return replacement; }, configurable: true });
        }
        let result, error;
        try { result = crypto.timingSafeEqual(left.value, right.value); }
        catch (caught) { error = {name: caught.name, code: caught.code ?? null}; }
        rows.push({route, shape, property, mode, result: result ?? null, error: error ?? null, reads});
      }
    }
  }
}
console.log(JSON.stringify({sameExport: cjs.timingSafeEqual === esm.timingSafeEqual, rows}));
`;

export const randomIntCallbackShapeProbe = String.raw`import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const cjs = require('node:crypto');
const esm = await import('node:crypto');
const rows = [];
for (const [route, crypto] of [['CJS', cjs], ['ESM', esm]]) {
  crypto.randomInt(1); // Finite warm-up; range has exactly one result.
  let returned = false;
  const callback = await new Promise((resolve, reject) => {
    const guard = setTimeout(() => reject(new Error('callback probe exceeded 500 ms')), 500);
    crypto.randomInt(1, function(error, value) {
      clearTimeout(guard);
      resolve({errorKind: error === undefined ? 'undefined' : error === null ? 'null' : typeof error,
        value, returned, argumentCount: arguments.length});
    });
    returned = true;
  });
  let validation;
  try { crypto.randomInt(NaN, 1, null); validation = {threw: false}; }
  catch (error) { validation = {threw: true, name: error.name, code: error.code,
    namesCallback: error.message.includes('"callback"'), namesMin: error.message.includes('"min"')}; }
  rows.push({route, callback, validation});
}
console.log(JSON.stringify({sameExport: cjs.randomInt === esm.randomInt, rows}));
`;

export const compressionInvalidWriteProbe = String.raw`const describe = (kind, value) => ({kind, ...(kind==='rejected'?{name:value?.name??null,code:value?.code??null}:value && typeof value.done==='boolean'?{done:value.done,valueBytes:value.value?.byteLength??null}:{})});
const outcome = p => Promise.resolve(p).then(v=>describe('fulfilled',v),e=>describe('rejected',e));
async function observe(p) { let timer; try { return await Promise.race([p,new Promise(r=>{timer=setTimeout(()=>r({kind:'pending'}),100);})]); } finally {clearTimeout(timer);} }
const rows=[];
for (const name of ['CompressionStream','DecompressionStream']) {
 const pair=new globalThis[name]('gzip');const reader=pair.readable.getReader();const writer=pair.writable.getWriter();
 const readerClosed=outcome(reader.closed), writerClosed=outcome(writer.closed), read=outcome(reader.read());
 const write=await observe(outcome(writer.write({})));
 const observed={write,reader:await observe(readerClosed),writer:await observe(writerClosed),read:await observe(read)};
 const cleanup=[await observe(outcome(reader.cancel())),await observe(outcome(writer.abort()))];
 if(cleanup.some(row=>row.kind==='pending'))process.exitCode=1;
 reader.releaseLock();writer.releaseLock();rows.push({name,observed,cleanup});
}
console.log(JSON.stringify(rows));
`;

export const compressionCrossRealmInputProbe = String.raw`const describe = (kind, value) => ({kind, ...(kind==='rejected'?{name:value?.name??null,code:value?.code??null}:value && typeof value.done==='boolean'?{done:value.done,valueBytes:value.value?.byteLength??null}:{})});
const outcome = p => Promise.resolve(p).then(v=>describe('fulfilled',v),e=>describe('rejected',e));
async function observe(p) { let timer; try { return await Promise.race([p,new Promise(r=>{timer=setTimeout(()=>r({kind:'pending'}),100);})]); } finally {clearTimeout(timer);} }
import vm from 'node:vm';
const input=vm.runInNewContext('new Uint8Array([97,98,99]).buffer');
const pair=new CompressionStream('gzip');const reader=pair.readable.getReader();const writer=pair.writable.getWriter();
const readerClosed=outcome(reader.closed), writerClosed=outcome(writer.closed), read=outcome(reader.read());
const write=await observe(outcome(writer.write(input)));
const cleanup=[await observe(outcome(reader.cancel())),await observe(outcome(writer.abort()))];
if(cleanup.some(row=>row.kind==='pending'))process.exitCode=1;
reader.releaseLock();writer.releaseLock();
console.log(JSON.stringify({write,cleanup}));
`;

export const callbackAfterProbe = String.raw`import test from 'node:test';
import assert from 'node:assert/strict';
const seen = [];
test('owner', t => {
  t.after((context, done) => {
    assert.equal(context.name, 'owner');
    assert.equal(typeof done, 'function');
    seen.push('after');
    done();
  });
});
test('after consumer', () => assert.deepEqual(seen, ['after']));
`;

export const callbackAfterDoneThrowProbe = String.raw`import test from 'node:test';
import assert from 'node:assert/strict';
const seen=[];
test('after-done owner',t=>{
  t.after((ctx,done)=>{
    seen.push('after');
    done();
    throw new Error('after-done sentinel');
  });
});
test('after-done consumer',()=>assert.deepEqual(seen,['after']));
`;
