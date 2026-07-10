import { readFileSync } from 'node:fs';

const path = process.argv[2];
if (!path) throw new Error('usage: delta-report.ts <node-stdlib-ab.json>');
const report = JSON.parse(readFileSync(path, 'utf8'));
const lines = [
  '## Node stdlib legacy → real benchmark delta',
  '',
  `Samples: ${report.protocol.iterations} measured + ${report.protocol.warmup} warmup, same sidecar and host.`,
  '',
  '| row | legacy p50 (ms) | real p50 (ms) | real/legacy | real guest RSS |',
  '| --- | ---: | ---: | ---: | ---: |',
];
for (const row of report.deltas) {
  lines.push(`| ${row.key} | ${row.legacyP50Ms ?? '-'} | ${row.realP50Ms ?? '-'} | ${row.ratio ?? '-'} | ${row.realGuestRssBytes ?? '-'} |`);
}
lines.push('', 'Native Node codec floor:', '');
for (const [name, row] of Object.entries(report.nativeNodeCodecFloor.rows) as Array<[string, any]>) {
  lines.push(`- ${name}: p50 ${row.p50Ms}ms, p99 ${row.p99Ms}ms, IQR ${row.iqrMs}ms (${row.samples} samples)`);
}
console.log(lines.join('\n'));
