import { getHardware, percentile, round } from '../lib/perf-utils.js';

const iterations = Math.max(5, Number(process.env.BENCH_NODE_CODEC_ITERATIONS ?? 9));
const warmup = Math.max(1, Number(process.env.BENCH_NODE_CODEC_WARMUP ?? 3));
const payload = 'AgentOS Node 24 codec floor — '.repeat(4096);

function summarize(samples: number[]) {
  const sorted = [...samples].sort((left, right) => left - right);
  return {
    samples: sorted.length,
    p50Ms: round(percentile(sorted, 50), 4),
    p99Ms: round(percentile(sorted, 99), 4),
    iqrMs: round(percentile(sorted, 75) - percentile(sorted, 25), 4),
    minMs: round(sorted[0], 4),
    maxMs: round(sorted.at(-1) ?? 0, 4),
  };
}

function measure(run: () => void): number[] {
  const samples: number[] = [];
  for (let index = 0; index < warmup + iterations; index++) {
    const start = process.hrtime.bigint();
    run();
    const elapsed = Number(process.hrtime.bigint() - start) / 1e6;
    if (index >= warmup) samples.push(elapsed);
  }
  return samples;
}

let sink = 0;
const utf8 = measure(() => {
  const encoded = Buffer.from(payload, 'utf8');
  sink ^= encoded.length;
  sink ^= encoded.toString('utf8').length;
});
const base64 = measure(() => {
  const encoded = Buffer.from(payload, 'utf8').toString('base64');
  sink ^= encoded.length;
  sink ^= Buffer.from(encoded, 'base64').length;
});
if (sink === Number.MIN_SAFE_INTEGER) throw new Error('unreachable codec sink');

console.log(JSON.stringify({
  schema: 1,
  node: process.version,
  payloadBytes: Buffer.byteLength(payload),
  iterations,
  warmup,
  hardware: getHardware(),
  rows: {
    utf8EncodeDecode: summarize(utf8),
    base64EncodeDecode: summarize(base64),
  },
}, null, 2));
