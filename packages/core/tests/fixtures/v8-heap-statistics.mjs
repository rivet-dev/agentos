import { createRequire } from "node:module";
import * as esmV8 from "node:v8";
const cjsV8 = createRequire(import.meta.url)("node:v8");
const before = cjsV8.getHeapStatistics();
const retained = [];
for (let index = 0; index < 25000; index += 1) {
	retained.push({ index, text: `${index}-`.padEnd(256, String(index % 10)) });
}
globalThis.__heapStatisticsRetained = retained;
const after = esmV8.getHeapStatistics();
console.log(JSON.stringify({ before, after, retainedCount: retained.length,
	sameBinding: cjsV8.getHeapStatistics === esmV8.getHeapStatistics }));
