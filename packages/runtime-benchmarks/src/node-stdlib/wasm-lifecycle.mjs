import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
	isMainThread,
	parentPort,
	Worker,
	workerData,
} from "node:worker_threads";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../..");
const scriptPath = fileURLToPath(import.meta.url);

function nowMs(start) {
	return Number(process.hrtime.bigint() - start) / 1e6;
}

function percentile(sorted, value) {
	return sorted[Math.min(sorted.length - 1, Math.ceil((value / 100) * sorted.length) - 1)];
}

function summarize(samples) {
	const sorted = [...samples].sort((left, right) => left - right);
	const round = (value) => Math.round(value * 10_000) / 10_000;
	return {
		samples: sorted.length,
		p50Ms: round(percentile(sorted, 50)),
		p99Ms: round(percentile(sorted, 99)),
		iqrMs: round(percentile(sorted, 75) - percentile(sorted, 25)),
		minMs: round(sorted[0]),
		maxMs: round(sorted.at(-1)),
	};
}

function importsFor(module) {
	const imports = {};
	for (const entry of WebAssembly.Module.imports(module)) {
		if (entry.kind !== "function") {
			throw new Error(`unsupported benchmark import ${entry.module}.${entry.name}: ${entry.kind}`);
		}
		imports[entry.module] ??= {};
		imports[entry.module][entry.name] = () => 0;
	}
	return imports;
}

async function workerSample() {
	let module = workerData.module;
	const compileStart = process.hrtime.bigint();
	if (!module) module = await WebAssembly.compile(workerData.bytes);
	const compileMs = workerData.module ? 0 : nowMs(compileStart);
	const instantiateStart = process.hrtime.bigint();
	await WebAssembly.instantiate(module, importsFor(module));
	parentPort.postMessage({ compileMs, instantiateMs: nowMs(instantiateStart) });
}

async function runWorker(data) {
	const started = process.hrtime.bigint();
	const worker = new Worker(new URL(import.meta.url), { workerData: data });
	const result = await new Promise((resolveResult, reject) => {
		worker.once("message", resolveResult);
		worker.once("error", reject);
		worker.once("exit", (code) => {
			if (code !== 0) reject(new Error(`wasm lifecycle worker exited ${code}`));
		});
	});
	await worker.terminate();
	return { ...result, wallMs: nowMs(started) };
}

async function childSample(modulePath) {
	const bytes = readFileSync(modulePath);
	const compileColdStart = process.hrtime.bigint();
	const module = await WebAssembly.compile(bytes);
	const compileColdMs = nowMs(compileColdStart);

	const compileWarmStart = process.hrtime.bigint();
	await WebAssembly.compile(bytes);
	const compileWarmMs = nowMs(compileWarmStart);

	const imports = importsFor(module);
	const instantiateColdStart = process.hrtime.bigint();
	await WebAssembly.instantiate(module, imports);
	const instantiateColdMs = nowMs(instantiateColdStart);
	const instantiateWarmStart = process.hrtime.bigint();
	await WebAssembly.instantiate(module, imports);
	const instantiateWarmMs = nowMs(instantiateWarmStart);

	const workerUnshared = await runWorker({ bytes });
	const workerShared = await runWorker({ module });
	return {
		compileColdMs,
		compileWarmMs,
		instantiateColdMs,
		instantiateWarmMs,
		workerUnshared,
		workerShared,
	};
}

function resolveModulePath() {
	const candidates = [
		process.env.NODE_STDLIB_WASM_BENCH_MODULE,
		resolve(repoRoot, "toolchain/c/build/openssl_handshake_smoke"),
		resolve(repoRoot, "packages/runtime-core/commands/true.wasm"),
		resolve(repoRoot, "crates/node-stdlib/vendor/test/fixtures/es-modules/globals.wasm"),
	].filter(Boolean);
	const path = candidates.find((candidate) => existsSync(candidate));
	if (!path) throw new Error("no WASM lifecycle benchmark module found");
	return path;
}

function runLane(modulePath, cacheEnabled, iterations, warmup) {
	const samples = [];
	for (let index = 0; index < warmup + iterations; index++) {
		const child = spawnSync(process.execPath, [
			cacheEnabled ? "--wasm-native-module-cache" : "--no-wasm-native-module-cache",
			scriptPath,
		], {
			env: {
				...process.env,
				WASM_LIFECYCLE_CHILD: "1",
				NODE_STDLIB_WASM_BENCH_MODULE: modulePath,
			},
			encoding: "utf8",
			maxBuffer: 16 * 1024 * 1024,
		});
		if (child.status !== 0) {
			throw new Error(`wasm lifecycle child failed (${child.status}): ${child.stderr}`);
		}
		if (index >= warmup) samples.push(JSON.parse(child.stdout));
	}
	const metric = (read) => summarize(samples.map(read));
	return {
		v8NativeModuleCache: cacheEnabled,
		compileCold: metric((sample) => sample.compileColdMs),
		compileWarmSameBytes: metric((sample) => sample.compileWarmMs),
		instantiateCold: metric((sample) => sample.instantiateColdMs),
		instantiateWarm: metric((sample) => sample.instantiateWarmMs),
		crossIsolateWithoutSharing: {
			compile: metric((sample) => sample.workerUnshared.compileMs),
			instantiate: metric((sample) => sample.workerUnshared.instantiateMs),
			wall: metric((sample) => sample.workerUnshared.wallMs),
		},
		crossIsolateWithModuleSharing: {
			instantiate: metric((sample) => sample.workerShared.instantiateMs),
			wall: metric((sample) => sample.workerShared.wallMs),
		},
	};
}

if (!isMainThread) {
	await workerSample();
} else if (process.env.WASM_LIFECYCLE_CHILD === "1") {
	process.stdout.write(`${JSON.stringify(await childSample(resolveModulePath()))}\n`);
} else {
	const modulePath = resolveModulePath();
	const bytes = readFileSync(modulePath);
	const iterations = Math.max(5, Number(process.env.BENCH_WASM_LIFECYCLE_ITERATIONS ?? 5));
	const warmup = Math.max(1, Number(process.env.BENCH_WASM_LIFECYCLE_WARMUP ?? 1));
	process.stdout.write(`${JSON.stringify({
		schema: 1,
		generatedAt: new Intl.DateTimeFormat("sv-SE", {
			timeZone: "America/Los_Angeles",
			year: "numeric",
			month: "2-digit",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
			second: "2-digit",
			hour12: false,
		}).format(new Date()).replace(" ", "T") + "-07:00",
		node: process.version,
		module: {
			name: basename(modulePath),
			bytes: bytes.byteLength,
			sha256: createHash("sha256").update(bytes).digest("hex"),
		},
		protocol: {
			iterations,
			warmup,
			isolate: "fresh Node process per sample; worker isolate pair per sample",
			cache: "V8 --wasm-native-module-cache on/off",
		},
		lanes: {
			cacheDisabled: runLane(modulePath, false, iterations, warmup),
			cacheEnabled: runLane(modulePath, true, iterations, warmup),
		},
	}, null, 2)}\n`);
}
