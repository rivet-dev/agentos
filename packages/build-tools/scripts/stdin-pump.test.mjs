import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { pathToFileURL } from "node:url";
import { Readable } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import { createRequire } from "node:module";
import { withOwnedEsbuild } from "./with-owned-esbuild.mjs";
const esbuildRequire = createRequire(process.env.AGENTOS_BUILD_TOOLS_PACKAGE ?? new URL("../package.json", import.meta.url));
const esbuildEntry = esbuildRequire.resolve("esbuild");
const { transform } = esbuildRequire("esbuild");

// Exercise the real stream implementation with manually settled kernel reads.
// No wall-clock timers, generated bridge bundles, or native assets are needed.
const { code } = await withOwnedEsbuild(esbuildEntry, async () => transform(
	await readFile(new URL("../bridge-src/builtins/stdin.ts", import.meta.url), "utf8"),
	{ loader: "ts", format: "cjs" },
));
function fixture({ external = false, registerHandle = () => {} } = {}) {
	const reads = [];
	const readSizes = [];
	const context = {
		module: { exports: {} }, TextDecoder, Buffer, queueMicrotask,
		__runtimeKernelStdin: !external, __runtimeStreamStdin: external,
		_registerHandle: registerHandle, _unregisterHandle() {},
		_kernelStdinRead: { apply(_receiver, args) { readSizes.push(args[0]); return new Promise((resolve) => reads.push(resolve)); } },
	};
	context.require = (name) => {
		if (name.endsWith("async-context.js")) return {
			getAsyncContextFrame() { return "stdin-owner"; },
			runInAsyncContextFrame(_frame, callback, receiver, args) { return Reflect.apply(callback, receiver, args); },
		};
		if (name.endsWith("global-exposure.js")) return {
			exposeCustomGlobal(name, value) { context[name] = value; },
			exposeMutableRuntimeStateGlobal(name, value) { context[name] = value; },
		};
		if (name.endsWith("prelude.js")) return { stringDecoderStdlibModuleNs: { StringDecoder } };
		if (name.endsWith("polyfills/index.js")) return { TextDecoder };
		if (name.endsWith("buffer-runtime.js")) return { import_buffer2: { Buffer } };
		if (name.endsWith("events.js")) return {};
		if (name.endsWith("process.js")) return {
			_getStdinIsTTY() { return false; }, isProcessExitError() { return false; },
			routeAsyncCallbackError(error) { return { handled: false, rethrow: error }; },
			scheduleAsyncRethrow(error) { throw error; },
		};
		throw Error(`unexpected stdin dependency: ${name}`);
	};
	vm.runInNewContext(code, context);
	return { reads, readSizes, api: context.module.exports, stdin: context.module.exports._stdin };
}
async function flush() { for (let i = 0; i < 8; i++) await Promise.resolve(); }
function data(bytes) { return { done: false, dataBase64: Buffer.from(bytes).toString("base64") }; }

test("data-listener pause stops read demand without EOF, resume preserves decoder", async () => {
	const { reads, stdin, api } = fixture();
	const chunks = []; let ended = 0; let closed = 0;
	stdin.setEncoding("utf8");
	stdin.on("end", () => ended++); stdin.on("close", () => closed++);
	stdin.on("data", (chunk) => { chunks.push(chunk); stdin.pause(); });
	assert.equal(reads.length, 1);
	// The euro sign is split across a pause/resume boundary.
	reads[0](data([0x61, 0xe2])); await flush();
	assert.deepEqual(chunks, ["a"]);
	assert.equal(reads.length, 1, "pause must not issue another kernel read");
	assert.equal(api.getStdinEnded(), false, "pause is not EOF");
	stdin.resume(); assert.equal(reads.length, 2);
	reads[1](data([0x82, 0xac])); await flush();
	assert.deepEqual(chunks, ["a", "€"]); assert.equal(reads.length, 2);
	stdin.resume(); assert.equal(reads.length, 3);
	reads[2]({ done: true }); await flush();
	assert.equal(ended, 1); assert.equal(closed, 1);
});

test("pause with a pending read buffers binary data and resumes without duplicate reads", async () => {
	const { reads, stdin, api } = fixture(); const chunks = [];
	stdin.on("data", (chunk) => chunks.push([...chunk]));
	stdin.pause(); assert.equal(reads.length, 1);
	reads[0](data([0, 255])); await flush();
	assert.deepEqual(chunks, []); assert.equal(reads.length, 1);
	assert.equal(api.getStdinEnded(), false);
	stdin.resume(); assert.deepEqual(chunks, [[0, 255]]);
	assert.equal(reads.length, 2);
	// Resume before an old read settles must reuse the in-flight pump.
	stdin.pause(); stdin.resume(); assert.equal(reads.length, 2);
	reads[1]({ done: true }); await flush();
	assert.equal(api.getStdinEnded(), true);
});


test("setEncoding decodes binary bytes buffered while paused, including partial characters", async () => {
	const native = spawnSync(process.execPath, ["-e", `
		const { Readable } = require('node:stream');
		const stream = new Readable({ read() {} });
		stream.push(Buffer.from([0xe2, 0x82]));
		stream.setEncoding('utf8');
		stream.push(Buffer.from([0xac])); stream.push(null);
		const chunks = []; stream.on('data', chunk => chunks.push(chunk));
		stream.on('end', () => console.log(JSON.stringify(chunks)));
	`], { encoding: "utf8", timeout: 3000, maxBuffer: 4096 });
	assert.equal(native.status, 0, native.stderr);
	const expected = JSON.parse(native.stdout);
	const { reads, stdin } = fixture(); const chunks = [];
	stdin.on("data", (chunk) => chunks.push(chunk)); stdin.pause();
	reads[0](data([0xe2, 0x82])); await flush();
	stdin.setEncoding("utf8"); stdin.resume();
	assert.deepEqual(chunks, [], "incomplete character remains in decoder");
	assert.equal(reads.length, 2);
	reads[1](data([0xac])); await flush();
	assert.deepEqual(chunks, expected);
	reads[2]({ done: true }); await flush();
});

test("operation retirement restarts a paused pump and fences its old completion", async () => {
	const { reads, stdin, api } = fixture(); const chunks = [];
	stdin.on("data", (chunk) => chunks.push([...chunk]));
	stdin.pause();
	api.retireStdinOperation();
	stdin.resume(); assert.equal(reads.length, 2, "new operation needs a fresh read");
	reads[0](data([99])); await flush();
	assert.deepEqual(chunks, [], "retired completion cannot deliver into new operation");
	stdin.resume(); assert.equal(reads.length, 2, "old finally cannot clear new running flag");
	reads[1](data([0, 255])); await flush();
	assert.deepEqual(chunks, [[0, 255]]);
	reads[2]({ done: true }); await flush();
});

test("operation retirement preserves buffered bytes and partial decoder state", async () => {
	const { reads, stdin, api } = fixture(); const chunks = [];
	stdin.setEncoding("utf8");
	stdin.on("data", (chunk) => { chunks.push(chunk); stdin.pause(); });
	reads[0](data([97, 0xe2])); await flush();
	assert.deepEqual(chunks, ["a"]);
	api.retireStdinOperation();
	stdin.resume(); assert.equal(reads.length, 2);
	reads[1](data([0x82, 0xac])); await flush();
	assert.deepEqual(chunks, ["a", "€"]);
	stdin.resume(); reads[2]({ done: true }); await flush();
});


test("readable demand starts one read and raw read returns bytes", async () => {
 const { reads, stdin } = fixture(); let notified = 0;
 stdin.on('readable', () => notified++);
 await flush(); assert.equal(reads.length, 1);
 reads[0](data([0,255,97])); await flush(); assert(notified > 0);
 const chunk = stdin.read(); assert(Buffer.isBuffer(chunk)); assert.deepEqual([...chunk], [0,255,97]);
});
test("read(0) starts bounded demand and consumes no bytes", async () => {
 const { reads, stdin } = fixture(); assert.equal(stdin.read(0), null); assert.equal(reads.length, 1);
 reads[0](data([0,255,97])); await flush();
 assert.equal(stdin.read(0), null); assert.equal(reads.length, 2, 'bounded prefetch may park the next read');
 assert.deepEqual([...stdin.read(2)], [0,255]);
});
test("read(size) waits for requested size and EOF drains short tail", async () => {
 const { reads, stdin } = fixture(); let ended=0; stdin.on('end', () => ended++);
 assert.equal(stdin.read(4), null); assert.equal(reads.length, 1);
 reads[0](data([1,2])); await flush(); assert.equal(stdin.read(4), null);
 assert.equal(reads.length, 2); reads[1](data([3,4,5])); await flush();
 assert.deepEqual([...stdin.read(4)], [1,2,3,4]);
 assert.equal(stdin.read(4), null); assert.equal(reads.length, 3);
 reads[2]({done:true}); await flush(); assert.equal(ended,0);
 assert.deepEqual([...stdin.read(4)], [5]); await flush(); assert.equal(ended,1);
});
test("idle readable consumer stops prefetch at the fixed high-water mark", async () => {
 const { reads, readSizes, stdin, api } = fixture(); stdin.on('readable', () => {});
 for(let i=0;i<4;i++) { assert.equal(reads.length,i+1); reads[i](data(Buffer.alloc(16384,i))); await flush(); }
 assert.equal(reads.length,4);assert.equal(api._stdinLiveBuffer.length,65536);
 assert.deepEqual(readSizes,[65536,49152,32768,16384]);
 await flush();assert.equal(reads.length,4);
});

test("readable listener suppresses flowing delivery until explicit read", async () => {
 const { reads, stdin } = fixture(); const seen=[];
 stdin.on('readable',()=>seen.push('readable'));
 stdin.on('data',chunk=>seen.push([...chunk]));
 reads[0](data([1,2])); await flush(); assert.deepEqual(seen,['readable']);
 assert.deepEqual([...stdin.read()], [1,2]); assert.deepEqual(seen,['readable',[1,2]]);
});
test("retired readable notification cannot pull or deliver in next operation", async () => {
 const { reads, stdin, api } = fixture(); let notified=0;
 stdin.on('readable',()=>notified++); api.retireStdinOperation();
 reads[0](data([99])); await flush(); assert.equal(notified,0);
 assert.equal(stdin.read(),null); assert.equal(reads.length,2);
 reads[1](data([0,255])); await flush(); assert.deepEqual([...stdin.read()],[0,255]);
});


const tick = () => new Promise(resolve => setImmediate(resolve));
const snapshot = value => value === null ? null : typeof value === 'string' ? value : [...value];
function random(seed) { return () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed; }; }

test('deterministic randomized binary splits and read sizes match native Readable', async () => {
 for (let seed=1; seed<=64; seed++) {
  const next=random(seed); const input=Buffer.from(Array.from({length: 1+next()%128},()=>next()%256));
  const native=new Readable({read(){}}); const {stdin,api}=fixture({external:true});
  native.on('readable',()=>{}); stdin.on('readable',()=>{});
  let nativeEnded=0, guestEnded=0;
  native.on('end',()=>nativeEnded++); stdin.on('end',()=>guestEnded++);
  const read = size => assert.deepEqual(snapshot(stdin.read(size)),snapshot(native.read(size)),`seed=${seed} size=${size}`);
  read(0);
  for(let offset=0; offset<input.length;) {
   const chunk=input.subarray(offset, offset+1+next()%11); offset+=chunk.length;
   native.push(chunk); api.stdinDispatch('stdin',data(chunk)); await tick();
   for(let attempt=0; attempt<3; attempt++) read(next()%5===0 ? undefined : next()%18);
   assert.equal(guestEnded,0);
  }
  native.push(null); api.stdinDispatch('stdin_end'); await tick();
  for(let attempt=0; attempt<256; attempt++) {
   const size=1+next()%17; const expected=native.read(size); const actual=stdin.read(size);
   assert.deepEqual(snapshot(actual),snapshot(expected),`EOF seed=${seed} size=${size}`);
   if(expected===null)break;
  }
  await tick(); assert.equal(nativeEnded,1); assert.equal(guestEnded,1);
 }
});

test('randomized UTF8 splits, midstream setEncoding and sized EOF reads match native', async () => {
 for(let seed=1;seed<=32;seed++) {
  const next=random(seed); const input=Buffer.from('a€🦊é漢字z');
  const native=new Readable({read(){}}); const {stdin,api}=fixture({external:true});
  native.push(input.subarray(0,2)); api.stdinDispatch('stdin',data(input.subarray(0,2)));
  native.setEncoding('utf8'); stdin.setEncoding('utf8');
  for(let offset=2;offset<input.length;) {
   const chunk=input.subarray(offset,offset+1+next()%4);offset+=chunk.length;
   native.push(chunk);api.stdinDispatch('stdin',data(chunk));await tick();
   const size=next()%4;assert.equal(stdin.read(size),native.read(size),`utf8 seed=${seed} size=${size}`);
  }
  native.push(null);api.stdinDispatch('stdin_end');await tick();
  assert.equal(stdin.read(),native.read());await tick();
 }
});

test('external streamed stdin preserves raw UTF8 strings and flushes partial decoder at EOF', async()=>{
 const {stdin,api}=fixture({external:true});api.stdinDispatch('stdin','€');
 assert.deepEqual([...stdin.read()],[0xe2,0x82,0xac]);
 stdin.setEncoding('utf8');api.stdinDispatch('stdin',data([0xe2]));api.stdinDispatch('stdin_end');
 assert.equal(stdin.read(),'�');await tick();
});

test('once readable listener can be removed before delivery',async()=>{
 const {stdin,reads}=fixture(); let called=0;const listener=()=>called++;
 stdin.once('readable',listener);stdin.removeListener('readable',listener);
 reads[0](data([1]));await tick();assert.equal(called,0);assert.equal(reads.length,1);
 assert.deepEqual([...stdin.read()],[1]);
});

test('switching readable to data preserves byte and EOF ordering like native',async()=>{
 async function exercise(stream,push,end) {
  const trace=[];const readable=()=>trace.push('readable');
  stream.on('readable',readable);stream.on('data',chunk=>trace.push([...chunk]));
  stream.on('end',()=>trace.push('end'));
  push(Buffer.from([1,2]));await tick();
  assert(!trace.some(Array.isArray));
  stream.removeListener('readable',readable);await tick();end();await tick();
  return trace.filter(value=>value!=='readable');
 }
 const native=new Readable({read(){}});
 const expected=await exercise(native,chunk=>native.push(chunk),()=>native.push(null));
 const {stdin,api}=fixture({external:true});
 const actual=await exercise(stdin,chunk=>api.stdinDispatch('stdin',data(chunk)),()=>api.stdinDispatch('stdin_end'));
 assert.deepEqual(actual,expected);
});


test('once readable removal automatically resumes data listeners like native',async()=>{
 async function exercise(stream,push,end) {
  const trace=[];stream.once('readable',()=>trace.push('readable'));
  stream.on('data',chunk=>trace.push([...chunk]));stream.on('end',()=>trace.push('end'));
  push(Buffer.from([1,2]));end();await tick();return trace;
 }
 const native=new Readable({read(){}});
 const expected=await exercise(native,c=>native.push(c),()=>native.push(null));
 const {stdin,api}=fixture({external:true});
 const actual=await exercise(stdin,c=>api.stdinDispatch('stdin',data(c)),()=>api.stdinDispatch('stdin_end'));
 assert.deepEqual(actual,expected);
});

test('read size coercion and upper bound match native without allocating large buffers',()=>{
 for(const size of [-1,0,1.9,'2',NaN,Infinity,1073741825]) {
  const native=new Readable({read(){}});native.push(Buffer.from('abcd'));
  const {stdin,api}=fixture({external:true});api.stdinDispatch('stdin','abcd');
  let expected,error;try{expected=native.read(size);}catch(reason){error=reason;}
  if(error)assert.throws(()=>stdin.read(size),{code:error.code});
  else assert.deepEqual(snapshot(stdin.read(size)),snapshot(expected));
 }
});

test('randomized kernel demand and short EOF tails match native Readable',async()=>{
 for(let seed=1;seed<=32;seed++) {
  const next=random(seed+100);const input=Buffer.from(Array.from({length:1+next()%64},()=>next()%256));
  const native=new Readable({read(){}});const {stdin,reads}=fixture();
  let offset=0,settled=0,eof=false,complete=false;
  for(let step=0;step<512;step++) {
   const size=next()%5===0?undefined:next()%16;
   const actual=stdin.read(size),expected=native.read(size);
   assert.deepEqual(snapshot(actual),snapshot(expected),`kernel seed=${seed} step=${step} size=${size}`);
   if(expected===null && eof) {complete=true;break;}
   if(reads.length>settled) {
    if(offset<input.length) {
     const chunk=input.subarray(offset,offset+1+next()%9);offset+=chunk.length;
     native.push(chunk);reads[settled++](data(chunk));
    } else {native.push(null);reads[settled++]({done:true});eof=true;}
   }
   await tick();
  }
  assert(complete,`bounded driver completed seed=${seed}`);assert.equal(offset,input.length);
 }
});

test('native pipe process.stdin readable delivers raw bytes used by source fixture',async()=>{
 const reference=spawnSync(process.execPath,['-e',`
  const bytes=[];process.stdin.on('readable',()=>{let c;while((c=process.stdin.read())!==null){if(!Buffer.isBuffer(c))throw Error('not Buffer');bytes.push(...c);}});
  process.stdin.on('end',()=>console.log(JSON.stringify(bytes)));
 `],{input:Buffer.from([0,255,97]),encoding:'utf8',timeout:3000,maxBuffer:4096});
 assert.equal(reference.status,0,reference.stderr);
 const {stdin,reads}=fixture();const bytes=[];
 stdin.on('readable',()=>{let c;while((c=stdin.read())!==null){assert(Buffer.isBuffer(c));bytes.push(...c);}});
 reads[0](data([0,255,97]));await tick();reads[1]({done:true});await tick();
 assert.deepEqual(bytes,JSON.parse(reference.stdout));
});


test('setEncoding uses Node decoder semantics across supported binary encodings',async()=>{
 for(const encoding of ['latin1','ascii','hex','base64','utf16le','utf8']) {
  const native=new Readable({read(){}});const {stdin,api}=fixture({external:true});
  native.setEncoding(encoding);stdin.setEncoding(encoding);
  for(const bytes of [[0,255,97],[0xe2,0x82],[0xac,0,0xff]]) {
   native.push(Buffer.from(bytes));api.stdinDispatch('stdin',data(bytes));
   assert.equal(stdin.read(),native.read(),encoding);
  }
  native.push(null);api.stdinDispatch('stdin_end');await tick();
  assert.equal(stdin.read(),native.read(),`${encoding} EOF`);
 }
});


test('read demand after pause restores the active handle while reusing the pending read',async()=>{
 let registered=0;const {stdin,reads}=fixture({registerHandle(){registered++;}});
 stdin.on('data',()=>{});assert.equal(registered,1);assert.equal(reads.length,1);
 stdin.pause();assert.equal(stdin.read(0),null);
 assert.equal(registered,2);assert.equal(reads.length,1);
 reads[0](data([1]));await tick();assert.deepEqual([...stdin.read()],[1]);
});


test('one read per readable event renews bounded demand through two chunks and EOF',async()=>{
 const native=new Readable({read(){}});const {stdin,reads}=fixture();
 const expected=[],actual=[];let nativeEnded=0,guestEnded=0;
 native.on('readable',()=>{const chunk=native.read();if(chunk!==null)expected.push([...chunk]);});
 stdin.on('readable',()=>{const chunk=stdin.read();if(chunk!==null)actual.push([...chunk]);});
 native.on('end',()=>nativeEnded++);stdin.on('end',()=>guestEnded++);
 assert.equal(reads.length,1);
 native.push(Buffer.from([0,255]));reads[0](data([0,255]));await tick();
 assert.deepEqual(actual,expected);assert.equal(reads.length,2,'successful consumption must renew demand');
 native.push(Buffer.from([97,98]));reads[1](data([97,98]));await tick();
 assert.deepEqual(actual,expected);assert.equal(reads.length,3,'next consumption must request EOF');
 native.push(null);reads[2]({done:true});await tick();
 assert.deepEqual(actual,[[0,255],[97,98]]);assert.equal(nativeEnded,1);assert.equal(guestEnded,1);
 assert.equal(reads.length,3,'EOF cannot renew demand');
});


test('sized single-read readable handler follows native refill and EOF event sequence',async()=>{
 const nativeRequests=[];const native=new Readable({highWaterMark:65536,read(size){nativeRequests.push(size);}});
 const {stdin,reads}=fixture();const expected=[],actual=[];let nativeEnded=0,guestEnded=0;
 native.on('readable',()=>{const chunk=native.read(1);expected.push(snapshot(chunk));});
 stdin.on('readable',()=>{const chunk=stdin.read(1);actual.push(snapshot(chunk));});
 native.on('end',()=>nativeEnded++);stdin.on('end',()=>guestEnded++);
 await tick();assert.equal(nativeRequests.length,1);assert.equal(reads.length,1);
 native.push(Buffer.from([1,2]));reads[0](data([1,2]));await tick();
 assert.deepEqual(actual,expected);assert.equal(reads.length,nativeRequests.length,'partial consumption permits the next read');
 native.push(Buffer.from([3,4]));reads[1](data([3,4]));await tick();
 assert.deepEqual(actual,expected);assert.equal(reads.length,nativeRequests.length,'next partial read can discover EOF');
 native.push(null);reads[2]({done:true});await tick();
 assert.deepEqual(actual,expected);assert.equal(guestEnded,nativeEnded);
 // A sized reader may leave bytes at EOF; compare native rather than assuming
 // another readable event must be emitted for every buffered byte.
 assert.deepEqual(snapshot(stdin.read()),snapshot(native.read()));await tick();
 assert.equal(guestEnded,nativeEnded);assert.equal(guestEnded,1);
});

test('idle readable prefetch and liveness reach the same bounded watermark as native',async()=>{
 const requests=[];const native=new Readable({highWaterMark:65536,read(size){requests.push(size);}});
 const {stdin,reads,api}=fixture();native.on('readable',()=>{});stdin.on('readable',()=>{});
 await tick();
 for(let i=0;i<4;i++) {
  assert.equal(requests.length,i+1);assert.equal(reads.length,i+1);
  const chunk=Buffer.alloc(16384,i);native.push(chunk);reads[i](data(chunk));await tick();
 }
 assert.equal(native.readableLength,65536);assert.equal(api._stdinLiveBuffer.length,native.readableLength);
 assert.equal(requests.length,4);assert.equal(reads.length,4);
});

test('one-byte consumption refills only the missing byte at the watermark',async()=>{
 const {stdin,reads,readSizes,api}=fixture();stdin.on('readable',()=>{});
 reads[0](data(Buffer.alloc(65536,7)));await tick();assert.equal(reads.length,1);
 for(let i=1;i<=8;i++) {
  assert.equal(stdin.read(1).length,1);assert.equal(readSizes[i],1);
  assert.equal(api._stdinLiveBuffer.length,65535);
  reads[i](data([i]));await tick();
  assert.equal(api._stdinLiveBuffer.length,65536);assert.equal(reads.length,i+1);
 }
});

test('explicit read above default watermark admits only the requested extra bytes',async()=>{
 const {stdin,reads,readSizes}=fixture();assert.equal(stdin.read(65540),null);
 reads[0](data(Buffer.alloc(65536,1)));await tick();
 assert.deepEqual(readSizes,[65536,4]);assert.equal(stdin.read(65540),null);
 reads[1](data([2,3,4,5]));await tick();assert.equal(reads.length,2);
 const chunk=stdin.read(65540);assert.equal(chunk.length,65540);
 assert.deepEqual([...chunk.subarray(65536)],[2,3,4,5]);
 assert.equal(readSizes[2],65536,'successful explicit read returns to the default refill bound');
});

test('injected handle-registration transport error propagates before admitting a stdin read',async()=>{
 const packageUrl = process.env.AGENTOS_BUILD_TOOLS_PACKAGE ? pathToFileURL(process.env.AGENTOS_BUILD_TOOLS_PACKAGE) : new URL('../package.json',import.meta.url);
 const {code:handlesCode}=await withOwnedEsbuild(esbuildEntry, async () => transform(await readFile(new URL('./bridge-src/builtins/active-handles.ts',packageUrl),'utf8'),{loader:'ts',format:'cjs'}));
 let reject=true;
 const context={module:{exports:{}},Error,queueMicrotask,require(name){
  if(name.endsWith('transport.js'))return {bridgeDispatchSync(){if(reject)throw new Error('EAGAIN');}};
  if(name.endsWith('global-exposure.js'))return {exposeCustomGlobal(){}};
  if(name.endsWith('stdin.js'))return {retireStdinOperation(){}};
  if(name.endsWith('process.js'))return {_exited:false};
  if(name.endsWith('timers.js'))return {_immediateEntries:new Map(),_timerEntries:new Map()};
  throw new Error(name);
 }};
 vm.runInNewContext(handlesCode,context);
 const {stdin,reads}=fixture({registerHandle:context.module.exports._registerHandle2});
 assert.throws(()=>stdin.read(),/ERR_RESOURCE_BUDGET_EXCEEDED/);
 assert.equal(reads.length,0);
 reject=false;assert.equal(stdin.read(),null);assert.equal(reads.length,1,'failure must not latch the pump started');
});


