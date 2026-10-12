import { spawnSync, execFileSync } from 'node:child_process';
import { closeSync, existsSync, mkdtempSync, openSync, readFileSync, rmdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

if (process.argv.length !== 6) {
  throw new Error('usage: node run-v8-default-vs-main.mjs <main-root> <candidate-root> <same-wasm-package> <output.json>');
}
const [mainRoot, candidateRoot, wasmPackage, output] = process.argv.slice(2).map(path => resolve(path));
if (!existsSync(wasmPackage)) throw new Error(`missing WASM package: ${wasmPackage}`);
const script = join(dirname(fileURLToPath(import.meta.url)), 'v8-default-vs-main.mjs');
const binaries = {
  main: join(mainRoot, 'target/release/agentos-native-sidecar'),
  candidate: join(candidateRoot, 'target/release/agentos-sidecar'),
};
for (const path of Object.values(binaries)) {
  if (!existsSync(path)) throw new Error(`missing release sidecar: ${path}`);
}
const sha256 = (path) => execFileSync('sha256sum', [path], { encoding: 'utf8' }).split(' ')[0];
const revision = (root, rev) => execFileSync('jj', ['log', '-r', rev, '--no-graph', '-T', 'commit_id ++ "\\n"'], { cwd: root, encoding: 'utf8' }).trim();
const mainRevision = revision(mainRoot, '@-');
if (!mainRevision.startsWith('c7462927ec45')) {
  throw new Error(`main workspace parent is ${mainRevision}, expected c7462927ec45`);
}
const order = ['main', 'candidate', 'candidate', 'main', 'main', 'candidate'];
const roots = { main: mainRoot, candidate: candidateRoot };
const runs = [];
const temporaryFiles = [];
const scratch = mkdtempSync(join(tmpdir(), 'agentos-v8-perf-'));
for (const [index, lane] of order.entries()) {
  const raw = join(scratch, `${index + 1}-${lane}.raw.json`);
  const log = join(scratch, `${index + 1}-${lane}.stderr.log`);
  console.error(`run ${index + 1}/${order.length}: ${lane}`);
  const stdout = openSync(raw, 'w');
  const stderr = openSync(log, 'w');
  const result = spawnSync(process.execPath, ['--expose-gc', script, roots[lane]], {
    env: { ...process.env, AGENTOS_SIDECAR_BIN: binaries[lane], PERF_WASM_PACKAGE: wasmPackage },
    stdio: ['ignore', stdout, stderr],
  });
  closeSync(stdout); closeSync(stderr);
  if (result.status !== 0) throw new Error(`${lane} run ${index + 1} failed; see ${log}`);
  const trial = JSON.parse(readFileSync(raw, 'utf8'));
  delete trial.root;
  delete trial.sidecar;
  runs.push({ lane, result: trial });
  temporaryFiles.push(raw, log);
}
const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const p95 = (values) => [...values].sort((a, b) => a - b)[Math.ceil(values.length * .95) - 1];
const columns = Object.keys(runs[0].result.stats);
const summary = {};
for (const column of columns) {
  const main = median(runs.filter(x => x.lane === 'main').map(x => x.result.stats[column].medianMs));
  const candidate = median(runs.filter(x => x.lane === 'candidate').map(x => x.result.stats[column].medianMs));
  const mainP95 = p95(runs.filter(x => x.lane === 'main').flatMap(x => x.result.stats[column].rawMs));
  const candidateP95 = p95(runs.filter(x => x.lane === 'candidate').flatMap(x => x.result.stats[column].rawMs));
  summary[column] = { mainMs: main, candidateMs: candidate, ratio: candidate / main,
    mainP95Ms: mainP95, candidateP95Ms: candidateP95, p95Ratio: candidateP95 / mainP95 };
}
for (const workload of ['idleVm', 'retainedJsContext']) {
  for (const column of ['beforeBytes', 'live12Bytes', 'disposedBytes', 'aggregateDeltaBytes', 'slopeBytesPerVm']) {
    const main = median(runs.filter(x => x.lane === 'main').map(x => x.result.memory[workload][column]));
    const candidate = median(runs.filter(x => x.lane === 'candidate').map(x => x.result.memory[workload][column]));
    summary[`${workload}.${column}`] = { mainBytes: main, candidateBytes: candidate, ratio: candidate / main };
  }
}
writeFileSync(output, JSON.stringify({
  objective: 'V8 default architecture against latest main',
  environment: 'Shared host with unrelated background services; no agentOS builds or tests during the timed runs.',
  order,
  settings: { iterations: Number(process.env.PERF_ITERATIONS || 12), warmup: Number(process.env.PERF_WARMUP || 3) },
  sources: { main: { revision: mainRevision, binarySha256: sha256(binaries.main) },
    candidate: { revision: revision(candidateRoot, '@'), binarySha256: sha256(binaries.candidate) },
    wasmPackage: { sha256: sha256(wasmPackage) } },
  summary, runs,
}, null, 2));
for (const path of temporaryFiles) unlinkSync(path);
rmdirSync(scratch);
console.error(`wrote ${output}`);
