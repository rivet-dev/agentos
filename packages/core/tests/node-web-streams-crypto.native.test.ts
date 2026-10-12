import { spawnSync } from "node:child_process";
import { afterEach, expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { compressionLifecycleProbe, crossRealmCryptoProbe } from "./fixtures/node-compression-crypto-probes.js";
import { timingSafeViewMetadataProbe, randomIntCallbackShapeProbe, compressionInvalidWriteProbe, compressionCrossRealmInputProbe } from "./fixtures/node-api-review-probes.js";

let vm: AgentOs | undefined;
afterEach(async () => { await vm?.dispose(); vm = undefined; });

async function compareProbe(source: string, requirePrelude = true): Promise<void> {
  const probe = requirePrelude ? `import {createRequire} from 'node:module';const require=createRequire(import.meta.url);${source}` : source;
  const host = spawnSync(process.execPath, ["--input-type=module", "-e", probe], {encoding:"utf8",timeout:10_000});
  expect(host.status, host.stderr).toBe(0);
  const expected = JSON.parse(host.stdout);
  vm = await AgentOs.create({defaultSoftware:false, permissions:{network:"deny"}});
  let stdout = "", stderr = "";
  const proc = await vm.process.spawn("node", ["--input-type=module", "-e", probe], {
    timeoutMs: 10_000,
    onStdout: c => { stdout += Buffer.from(c).toString(); },
    onStderr: c => { stderr += Buffer.from(c).toString(); },
  });
  expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
  expect(JSON.parse(stdout)).toEqual(expected);
}

test("Web Streams globals share the CJS and ESM module constructors", async () => {
  await compareProbe(`
const web = await import('node:stream/web');
const cjsWeb = require('node:stream/web');
const names = ['ByteLengthQueuingStrategy','CountQueuingStrategy','ReadableStreamDefaultReader','ReadableStreamBYOBReader','ReadableStreamBYOBRequest','ReadableStreamDefaultController','ReadableByteStreamController','WritableStreamDefaultWriter','WritableStreamDefaultController','TransformStreamDefaultController'];
const shape = names.map(name => [name, typeof globalThis[name], web[name] === globalThis[name], cjsWeb[name] === globalThis[name]]);
const source = new ReadableStream({start(controller) {controller.enqueue('ok');controller.close();}}, new CountQueuingStrategy({highWaterMark:1}));
const reader = source.getReader();
console.log(JSON.stringify({shape, reader:reader instanceof ReadableStreamDefaultReader, read:await reader.read(), bytes:new ByteLengthQueuingStrategy({highWaterMark:4}).size(new Uint8Array(3))}));`);
}, 20_000);

test("crypto timingSafeEqual compares byte views and preserves Node errors", async () => {
  await compareProbe(`
const crypto = await import('node:crypto');
const cjsCrypto = require('node:crypto');
const error = (fn) => { try { fn(); return null; } catch (e) { return [e.name, e.code]; } };
const backing = new Uint8Array([9, 1, 2, 9]);
const slice = new DataView(backing.buffer, 1, 2);
const compare = crypto.timingSafeEqual;
console.log(JSON.stringify({sameExport:compare === cjsCrypto.timingSafeEqual, same:compare(slice,new Uint8Array([1,2])), different:compare(slice,new Uint8Array([1,3])), empty:compare(new ArrayBuffer(0),new Uint8Array()), length:error(()=>compare(new Uint8Array(1),new Uint8Array(2))), type:error(()=>compare('ab',new Uint8Array(2))),spoofs:[{[Symbol.toStringTag]:'ArrayBuffer',byteLength:2},Object.create(ArrayBuffer.prototype),new Proxy(new ArrayBuffer(2),{})].map(value=>error(()=>compare(value,new Uint8Array(2))))}));`);
}, 20_000);

test("compression globals stream all standard formats and reject malformed input", async () => {
  await compareProbe(`
const web = await import('node:stream/web');
const results=[];
for(const format of ['gzip','deflate','deflate-raw']) {
 const compression=new CompressionStream(format);
 const chunks=[new TextEncoder().encode('first '),new TextEncoder().encode('second')];
 const source=new ReadableStream({start(c){for(const chunk of chunks)c.enqueue(chunk);c.close();}});
 const response=new Response(source.pipeThrough(compression).pipeThrough(new DecompressionStream(format)));
 results.push([format,await response.text(),compression instanceof web.CompressionStream]);
}
let invalid=false;
try{new CompressionStream('invalid');}catch(e){invalid=e instanceof TypeError;}
let corrupt=false;
try{await new Response(new Blob(['invalid gzip']).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer();}catch(e){corrupt=true;}
console.log(JSON.stringify({results,invalid,corrupt}));`);
}, 20_000);

test("net.Server uses normal EventEmitter listener removal and ordering", async () => {
  await compareProbe(`
const {createServer}=require('node:net');const {EventEmitter}=require('node:events');
const server=createServer();const calls=[];
const same=()=>calls.push('same');
server.on('probe',same);server.once('probe',same);server.removeListener('probe',same);server.emit('probe');
server.off('probe',same);server.emit('probe');
const once=()=>calls.push('once');server.once('probe',once);server.off('probe',once);server.emit('probe');
console.log(JSON.stringify({calls,isEmitter:server instanceof EventEmitter,aliases:server.off===server.removeListener,listeners:server.listenerCount('probe')}));`);
}, 20_000);

test("node --test executes an explicitly named file without a discovery suffix", async () => {
  vm = await AgentOs.create({defaultSoftware:false, permissions:{network:"deny"}});
  await vm.writeFile("/tmp/explicit-entry.mjs", `import test from 'node:test'; import assert from 'node:assert/strict'; test('explicit case',()=>assert.equal(2+2,4));`);
  let stdout = "", stderr = "";
  const proc = await vm.process.spawn("node", ["--test", "/tmp/explicit-entry.mjs"], {
    timeoutMs: 10_000,
    onStdout: c => { stdout += Buffer.from(c).toString(); },
    onStderr: c => { stderr += Buffer.from(c).toString(); },
  });
  expect((await vm.process.wait(proc.pid)).exitCode, stderr).toBe(0);
  expect(stdout).toContain("ok 1 - explicit case");
  expect(stdout).toContain("# tests 1");
}, 20_000);


test("Performance exposes its runtime-owned clock through global, CJS and ESM constructors", async () => {
  await compareProbe(`
const cjs = require('node:perf_hooks');
const esm = await import('node:perf_hooks');
let illegal;try{new Performance();}catch(e){illegal=[e.name,e.code];}
performance.mark('constructor-probe');
const entry=performance.getEntriesByName('constructor-probe')[0];
console.log(JSON.stringify({global:performance instanceof Performance,cjs:cjs.Performance===Performance,esm:esm.Performance===Performance,identity:cjs.performance===performance,tag:Object.prototype.toString.call(performance),clock:Number.isFinite(performance.now()),json:performance.toJSON().timeOrigin===performance.timeOrigin,jsonEpoch:Number.isFinite(JSON.parse(JSON.stringify(performance)).timeOrigin),entry:{name:entry.name,type:entry.entryType},illegal}));
performance.clearMarks('constructor-probe');`);
}, 20_000);

test("crypto.randomInt exposes unbiased bounded sync and asynchronous callback APIs", async () => {
  await compareProbe(`
const cjs=require('node:crypto');const esm=await import('node:crypto');
const error=(args)=>{try{cjs.randomInt(...args);return null;}catch(e){return [e.name,e.code];}};
const ranges=[[0,1],[-8,8],[0,1e11],[Number.MAX_SAFE_INTEGER-3,Number.MAX_SAFE_INTEGER]];
const valid=ranges.map(([min,max])=>Array.from({length:8},()=>cjs.randomInt(min,max)).every(value=>Number.isSafeInteger(value)&&value>=min&&value<max));
let returned=false;const deferred=await new Promise((resolve,reject)=>{cjs.randomInt(7,(error,value)=>error?reject(error):resolve({returned,valid:value>=0&&value<7}));returned=true;});
console.log(JSON.stringify({identity:cjs.randomInt===esm.randomInt,valid,deferred,errors:[[0],[-1],[1.5],['2'],[0,2**48],[5,1],[0,1,null]].map(error)}));`);
}, 20_000);

test("compression cancellation and abort settle both sides before cleanup", async () => {
  await compareProbe(compressionLifecycleProbe);
}, 20_000);

test("timingSafeEqual accepts genuine buffers and views from another realm", async () => {
  await compareProbe(crossRealmCryptoProbe);
}, 20_000);

test("timingSafeEqual reads genuine view slots without invoking shadowed metadata", async () => {
  await compareProbe(timingSafeViewMetadataProbe, false);
}, 20_000);

test("randomInt preserves callback success arguments and validation precedence", async () => {
  await compareProbe(randomIntCallbackShapeProbe, false);
}, 20_000);

test("invalid compression writes settle both sides before explicit cleanup", async () => {
  await compareProbe(compressionInvalidWriteProbe, false);
}, 20_000);

test("compression accepts a genuine ArrayBuffer from another realm", async () => {
  await compareProbe(compressionCrossRealmInputProbe, false);
}, 20_000);
