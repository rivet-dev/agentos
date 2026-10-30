import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { cpus, totalmem } from 'node:os';

const root = process.argv[2];
if (!root) throw new Error('usage: node --expose-gc v8-default-vs-main.mjs <checkout>');
const { AgentOs } = await import(pathToFileURL(join(root, 'packages/core/dist/index.js')));
const requireFromCore = createRequire(join(root, 'packages/core/package.json'));
const { z } = requireFromCore('zod');
const iterations = Number(process.env.PERF_ITERATIONS || 12);
const warmup = Number(process.env.PERF_WARMUP || 3);
if (!Number.isInteger(iterations) || iterations < 1 || !Number.isInteger(warmup) || warmup < 0) {
  throw new Error('PERF_ITERATIONS must be positive and PERF_WARMUP must be nonnegative');
}
const now = () => performance.now();
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const percentile = (values, fraction) => [...values].sort((a,b)=>a-b)[Math.ceil(fraction*values.length)-1];
const stats = (values) => ({ samples: values.length, medianMs: median(values), p95Ms: percentile(values, .95), meanMs: values.reduce((a,b)=>a+b,0)/values.length, rawMs: values });
const slope = (values, start = 3) => {
  const points = values.slice(start).map((rss, index) => [start + index, rss]);
  const xMean = points.reduce((sum, [x]) => sum + x, 0) / points.length;
  const yMean = points.reduce((sum, [, y]) => sum + y, 0) / points.length;
  const numerator = points.reduce((sum, [x, y]) => sum + (x - xMean) * (y - yMean), 0);
  const denominator = points.reduce((sum, [x]) => sum + (x - xMean) ** 2, 0);
  return numerator / denominator;
};
const gc = async () => { global.gc?.(); global.gc?.(); await new Promise(r=>setTimeout(r,50)); };
function readProc(path, pid) {
  try { return readFileSync(path, 'utf8'); }
  catch (error) {
    if (pid !== process.pid && (error.code === 'ENOENT' || error.code === 'ESRCH')) return null;
    throw error;
  }
}
function rss(pid) {
  const statm = readProc(`/proc/${pid}/statm`, pid);
  if (statm === null) return null;
  const pages = Number(statm.trim().split(/\s+/)[1]);
  if (!Number.isSafeInteger(pages) || pages < 0) throw new Error(`invalid /proc/${pid}/statm RSS`);
  return pages * 4096;
}
function treeRss() {
  const queue = [process.pid]; const seen = new Set(); let bytes = 0;
  while (queue.length) {
    const pid = queue.pop(); if (seen.has(pid)) continue; seen.add(pid);
    const processRss = rss(pid); if (processRss === null) continue; bytes += processRss;
    const children = readProc(`/proc/${pid}/task/${pid}/children`, pid);
    if (children?.trim()) queue.push(...children.trim().split(/\s+/).map(Number));
  }
  return bytes;
}
function check(result, label) { if (result.outcome !== 'succeeded') throw new Error(`${label}: ${result.stderr || result.outcome}`); return result; }
const options = (sidecar, extra = {}) => ({ defaultSoftware: false, sidecar: { kind: 'explicit', handle: sidecar }, ...extra });

const samples = { vmCreate: [], firstJs: [], coldJs: [], coldJsEndToEnd: [], warmJs: [], serialJs: [], concurrentJs: [], hostCallbacks: [] };
const startup = [];
for (let i=0; i<warmup+iterations; i++) {
  const t0=now(); const sidecar=await AgentOs.createSidecar(); const sidecarHandleMs=now()-t0;
  const t1=now(); const vm=await AgentOs.create(options(sidecar)); const vmMs=now()-t1;
  const t2=now(); check(await vm.javascript.execute('console.log(42)'), 'first JS'); const firstJs=now()-t2;
  const t3=now(); check(await vm.javascript.execute('console.log(42)'), 'warm JS'); const warmJs=now()-t3;
  if(i>=warmup) {
    startup.push({sidecarHandleMs, vmMs, firstJs, warmJs});
    samples.vmCreate.push(vmMs); samples.firstJs.push(firstJs);
    samples.coldJs.push(vmMs+firstJs);
    samples.coldJsEndToEnd.push(sidecarHandleMs+vmMs+firstJs);
    samples.warmJs.push(warmJs);
  }
  await vm.dispose(); await sidecar.dispose();
}

const sidecar = await AgentOs.createSidecar();
const vm = await AgentOs.create(options(sidecar));
check(await vm.javascript.execute('console.log(42)'), 'throughput prewarm');
for(let i=0; i<warmup+iterations; i++) {
  const t=now(); for(let j=0;j<8;j++) check(await vm.javascript.execute('console.log(42)'), 'serial JS');
  if(i>=warmup) samples.serialJs.push((now()-t)/8);
}
for(let i=0; i<warmup+iterations; i++) {
  const t=now(); const results=await Promise.all(Array.from({length:8},()=>vm.javascript.execute('console.log(42)')));
  for(const result of results) check(result, 'concurrent JS');
  if(i>=warmup) samples.concurrentJs.push((now()-t)/8);
}
await vm.dispose();

const calls = 30;
const bridge = { ping: { inputSchema: z.object({ value: z.number() }), execute: ({value}) => ({value:value+1}) } };
const bindingVm = await AgentOs.create(options(sidecar, {
  hostFunctions: { bridge },
  permissions: { fs:'allow', network:'allow', childProcess:'allow', process:'allow', env:'allow', hostFunction:'allow' },
}));
const callbackCode = `for (let i=0;i<${calls};i++){ const r=await bridge.ping({value:i}); if(r.value!==i+1) throw new Error('bad callback'); }`;
for(let i=0;i<warmup+iterations;i++) {
  const t=now(); check(await bindingVm.javascript.execute(callbackCode), 'host callback');
  if(i>=warmup) samples.hostCallbacks.push((now()-t)/calls);
}
await bindingVm.dispose();

// Use identical generated guest bytes on both runtimes.
if (process.env.PERF_WASM_PACKAGE) {
  samples.firstWasm = []; samples.warmWasm = []; samples.wasmFs = [];
  const software = [{ packagePath: process.env.PERF_WASM_PACKAGE }];
  for(let i=0;i<warmup+iterations;i++) {
    const wasmVm=await AgentOs.create(options(sidecar, {software}));
    const run=async(command,args=[])=>{const r=await wasmVm.execArgv(command,args); if(r.exitCode!==0)throw new Error(`WASM ${command}: ${JSON.stringify(r)}`);};
    let t=now(); await run('true'); const first=now()-t;
    t=now(); await run('true'); const warm=now()-t;
    t=now(); await run('ls',['/tmp']); const fs=now()-t;
    if(i>=warmup){samples.firstWasm.push(first); samples.warmWasm.push(warm); samples.wasmFs.push(fs);}
    await wasmVm.dispose();
  }
}

async function memoryTrial(init) {
  // Settle old execution isolates and the sidecar pool before each workload.
  await new Promise(r=>setTimeout(r,2000)); await gc();
  const rssByLiveVm=[treeRss()];
  const vms=[];
  for(let i=0;i<12;i++) {
    const one=await AgentOs.create(options(sidecar));
    await init(one); vms.push(one);
    await new Promise(r=>setTimeout(r,500)); await gc();
    rssByLiveVm.push(treeRss());
  }
  await Promise.all(vms.map(x=>x.dispose()));
  await new Promise(r=>setTimeout(r,2000)); await gc();
  const disposedBytes=treeRss();
  const beforeBytes=rssByLiveVm[0];
  const live12Bytes=rssByLiveVm.at(-1);
  return { beforeBytes, live12Bytes, disposedBytes, rssByLiveVm,
    aggregateDeltaBytes:live12Bytes-beforeBytes,
    slopeBytesPerVm:slope(rssByLiveVm),
    reclaimedBytes:live12Bytes-disposedBytes };
}
const memory = {
  idleVm: await memoryTrial(async () => {}),
  retainedJsContext: await memoryTrial(async (one) => {
    await one.createContext('benchmark');
    check(await one.javascript.execute('globalThis.__perfHold = new Uint8Array(65536)', {contextId:'benchmark'}), 'retained context');
    check(await one.javascript.execute('if (globalThis.__perfHold?.length !== 65536) throw new Error("context lost")', {contextId:'benchmark'}), 'context retained');
  }),
};
await sidecar.dispose();
console.log(JSON.stringify({root, sidecar:process.env.AGENTOS_SIDECAR_BIN, iterations, warmup,
  machine:{node:process.version, arch:process.arch, platform:process.platform, cpuCount:cpus().length, memoryBytes:totalmem()},
  startup, stats:Object.fromEntries(Object.entries(samples).map(([k,v])=>[k,stats(v)])),
  memory}, null, 2));
