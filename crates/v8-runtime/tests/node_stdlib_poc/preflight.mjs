// Fast local pre-flight for the POC bootstrap: runs bootstrap.js + checks.js in
// a bare `vm` context (no node globals) to approximate the agentOS isolate.
// Not part of the Rust test — dev iteration aid only. Run:
//   node crates/v8-runtime/tests/node_stdlib_poc/preflight.mjs
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import vm from 'node:vm';

const NODE_SRC = process.env.NODE_SRC_DIR ?? '/home/nathan/misc/node';
const HERE = new URL('.', import.meta.url).pathname;

const sources = {};
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (name.endsWith('.js')) {
      const id = relative(join(NODE_SRC, 'lib'), p).replace(/\.js$/, '');
      sources[id] = readFileSync(p, 'utf8');
    }
  }
}
walk(join(NODE_SRC, 'lib'));

const constants = JSON.parse(readFileSync(join(HERE, 'constants.json'), 'utf8'));
const bootstrap = readFileSync(join(HERE, 'bootstrap.js'), 'utf8');
const checks = readFileSync(join(HERE, 'checks.js'), 'utf8');

const ctx = vm.createContext(Object.create(null));
ctx.globalThis = ctx;
vm.runInContext('globalThis.__nodeSources = ' + JSON.stringify(sources), ctx);
vm.runInContext('globalThis.__nodeConstants = ' + JSON.stringify(constants), ctx);
// Stage 2.5 (optional): simdutf wasm; absent → JS codec fallback.
const wasmPath = process.env.AGENTOS_SIMDUTF_POC_WASM ?? join(HERE, 'simdutf/build/simdutf-poc.wasm');
try {
  const b64 = readFileSync(wasmPath).toString('base64');
  vm.runInContext(`globalThis.__pocSimdutfWasmBase64 = ${JSON.stringify(b64)}`, ctx);
} catch {
  console.error(`note: no simdutf wasm at ${wasmPath}; JS codec fallback`);
}
try {
  vm.runInContext(bootstrap, ctx, { filename: 'bootstrap.js' });
  vm.runInContext(checks, ctx, { filename: 'checks.js' });
  // Stage 3: wait for the async checks (fs callbacks/promises/streams).
  await vm.runInContext('globalThis.__pocAsync', ctx);
  console.log(vm.runInContext('globalThis.__pocResult', ctx));
} catch (err) {
  console.error('PREFLIGHT FAILED:', err.stack ?? err);
  process.exit(1);
}
