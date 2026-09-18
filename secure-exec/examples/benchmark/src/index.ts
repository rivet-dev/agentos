// Compares where the time goes when running the same guest code in Secure Exec
// (V8 with JIT, separate process), quickjs-emscripten (interpreter in
// WebAssembly, same thread), and Vercel Run (interpreter in WebAssembly, worker
// thread). Each workload is a function body that returns a small JSON value, so
// no engine pays to move data across its boundary.
import { getQuickJS } from "quickjs-emscripten";
import { run } from "run";
import { createVm, evaluate, shutdown } from "secure-exec";

// docs:start workloads
const workloads: Record<string, string> = {
	// Fixed cost of one call: nothing to compute.
	baseline: "return 1;",
	// Build, filter, group, sort, and serialize 50,000 records.
	records: `
		const rows = [];
		for (let i = 0; i < 50000; i++) rows.push({ id: i, group: i % 97, price: (i * 31) % 1000, name: "item-" + i });
		const byGroup = new Map();
		for (const row of rows.filter((r) => r.price > 250)) {
			const entry = byGroup.get(row.group) ?? { count: 0, total: 0 };
			entry.count++; entry.total += row.price; byGroup.set(row.group, entry);
		}
		const top = [...byGroup.entries()].map(([group, e]) => ({ group, avg: e.total / e.count })).sort((a, b) => b.avg - a.avg).slice(0, 5);
		return JSON.parse(JSON.stringify(top)).length;`,
	// A prime sieve and a tight arithmetic loop.
	numeric: `
		let count = 0;
		const limit = 300000;
		const sieve = new Uint8Array(limit + 1);
		for (let i = 2; i <= limit; i++) { if (!sieve[i]) { count++; for (let j = i * 2; j <= limit; j += i) sieve[j] = 1; } }
		let acc = 0;
		for (let i = 0; i < 5000000; i++) acc = (acc + i * i) % 1000003;
		return count + acc;`,
	// Build about 1 MB of text, split it, count words, and run a regex replace.
	text: `
		let text = "";
		for (let i = 0; i < 20000; i++) text += "the quick brown fox " + i + " jumps over the lazy dog\\n";
		const words = text.split(/\\s+/).filter(Boolean);
		const freq = {};
		for (const w of words) freq[w] = (freq[w] ?? 0) + 1;
		return Object.keys(freq).length + text.replace(/o/g, "0").length;`,
};
// docs:end workloads

const expression = (body: string) => `(() => {${body}})()`;
const QuickJS = await getQuickJS();
const vm = await createVm();

// docs:start engines
const engines: Record<string, (body: string) => Promise<unknown>> = {
	"secure-exec, one-shot (fresh VM per call)": async (body) => {
		const result = await evaluate(expression(body));
		return result.outcome === "succeeded" ? result.value : result.outcome;
	},
	"secure-exec, existing VM": async (body) => {
		const result = await vm.javascript.evaluate(expression(body));
		return result.outcome === "succeeded" ? result.value : result.outcome;
	},
	"quickjs-emscripten (fresh context per call)": async (body) => {
		const context = QuickJS.newContext();
		try {
			const handle = context.unwrapResult(context.evalCode(expression(body)));
			const value = context.dump(handle);
			handle.dispose();
			return value;
		} finally {
			context.dispose();
		}
	},
	"vercel run": async (body) => {
		const result = await run({ source: body });
		return result.status === "completed" ? result.value : result.status;
	},
};
// docs:end engines

async function medianMs(call: () => Promise<unknown>, runs = 5) {
	const samples: number[] = [];
	for (let i = 0; i < runs; i++) {
		const started = performance.now();
		await call();
		samples.push(performance.now() - started);
	}
	return samples.sort((a, b) => a - b)[Math.floor(runs / 2)];
}

const names = Object.keys(workloads);
console.log(["engine", ...names].join("\t"));
for (const [engine, execute] of Object.entries(engines)) {
	await execute(workloads.baseline); // Warm up before measuring.
	const row = [engine];
	for (const name of names) {
		row.push(
			`${(await medianMs(() => execute(workloads[name]))).toFixed(1)}ms`,
		);
	}
	console.log(row.join("\t"));
}

await vm.dispose();
await shutdown();
