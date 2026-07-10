import { execFileSync } from 'node:child_process';
import { basename, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { getHardware, round } from '../lib/perf-utils.js';
import { formatPacificIso } from '../lib/vm.js';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const iterations = Math.max(5, Number(process.env.BENCH_NODE_STDLIB_ITERATIONS ?? 7));
const warmup = Math.max(1, Number(process.env.BENCH_NODE_STDLIB_WARMUP ?? 2));
const selectedOps = process.env.BENCH_NODE_STDLIB_OPS ??
  'cpu_loop,fs_read_small,fs_read_big,stat_storm,readdir_big,stream_copy_big,require_100_small,import_npm_package,pass_through_big';

function runJson(script: string, args: string[], env: NodeJS.ProcessEnv): any {
  const stdout = execFileSync('pnpm', [
    '--silent', '--dir', packageRoot, 'exec', 'tsx', resolve(packageRoot, script), ...args,
  ], {
    cwd: packageRoot,
    env: { ...process.env, ...env },
    encoding: 'utf8',
    maxBuffer: 128 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  return JSON.parse(stdout);
}

function lane(flavor: 'legacy' | 'real') {
  const env = {
    AGENTOS_JS_STDLIB: flavor,
    BENCH_ITERATIONS: String(iterations),
    BENCH_WARMUP: String(warmup),
    BENCH_SHARED_VM: '1',
    BENCH_FAMILIES: 'control,fs,modules,pipes',
    BENCH_OP_FILTER: selectedOps,
  };
  const matrix = runJson('src/run-all.ts', [], env);
  if (matrix.sidecar?.path) matrix.sidecar.path = basename(matrix.sidecar.path);
  const bridge = runJson('src/focused/sync-bridge-floor.bench.ts', [
    `--iterations=${iterations}`,
    `--warmup=${warmup}`,
    '--call-counts=1',
    '--payload-bytes=0',
  ], env);
  const coldstart = runJson('coldstart.bench.ts', [], {
    ...env,
    BENCH_BATCH_SIZES: '1',
    BENCH_SCENARIOS: 'shared-sidecar',
    BENCH_COLDSTART_STDLIB: '1',
    BENCH_MAX_LIVE_RUNTIMES: '1',
  });
  return { flavor, matrix, bridge, coldstart };
}

const codec = runJson('src/node-stdlib/native-codec-floor.ts', [], {});
const wasmLifecycle = runJson('src/node-stdlib/wasm-lifecycle.mjs', [], {});
const legacy = lane('legacy');
const real = lane('real');

const legacyRows = new Map(legacy.matrix.latency.map((row: any) => [`${row.family}/${row.op}`, row]));
const deltas = real.matrix.latency.map((row: any) => {
  const key = `${row.family}/${row.op}`;
  const before = legacyRows.get(key) as any;
  const legacyP50Ms = before?.layers?.guest?.p50 ?? null;
  const realP50Ms = row.layers?.guest?.p50 ?? null;
  return {
    key,
    legacyP50Ms,
    realP50Ms,
    ratio: legacyP50Ms && realP50Ms ? round(realP50Ms / legacyP50Ms, 4) : null,
    realGuestRssBytes: row.layers?.guest?.memBytes ?? null,
  };
});

console.log(JSON.stringify({
  schema: 1,
  generatedAt: formatPacificIso(new Date()),
  hardware: getHardware(),
  protocol: {
    iterations,
    warmup,
    selectedOps: selectedOps.split(','),
    filesystem: 'VM /tmp on the same in-memory VFS for both flavors; native Node codec is host-memory only',
    fairness: 'same sidecar binary, host, sample counts, warmup, op order, and payloads',
  },
  nativeNodeCodecFloor: codec,
  wasmLifecycle,
  lanes: { legacy, real },
  deltas,
}, null, 2));
