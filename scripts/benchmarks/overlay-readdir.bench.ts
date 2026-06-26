/**
 * Overlay readdir benchmark.
 *
 * Isolates Agent OS' JS overlay filesystem readdir behavior from VM creation,
 * WASM process startup, and host mount-table overhead.
 */

import {
	createInMemoryFileSystem,
	createInMemoryLayerStore,
	createSnapshotExport,
	type LayerStore,
	type SnapshotLayerHandle,
	type VirtualDirEntry,
	type VirtualFileSystem,
} from "@rivet-dev/agentos-core";
import { getHardware, printTable, round, stats } from "./lib/perf-utils.js";

type ReaddirMode = "plain" | "withFileTypes";

interface OverlayCaseResult {
	scenario: string;
	entryCount: number;
	mode: ReaddirMode;
	iterations: number;
	warmup: number;
	opsPerSample: number;
	expectedCount: number;
	returnedCount: number;
	payloadBytes: number;
	readdir: ReturnType<typeof stats>;
	sampleBatchMs: ReturnType<typeof stats>;
	msPerEntry: number;
	rawPerOpMs: number[];
	rawBatchMs: number[];
}

interface CaseFixture {
	filesystem: VirtualFileSystem;
	expectedNames: string[];
	dispose(): void;
}

function parseArgs(): {
	iterations: number;
	warmup: number;
	entryCounts: number[];
	modes: ReaddirMode[];
	opsPerSample: number;
} {
	const value = (name: string) =>
		process.argv.find((arg) => arg.startsWith(`--${name}=`))?.split("=")[1];
	const iterations = Number(value("iterations") ?? 20);
	const warmup = Number(value("warmup") ?? 5);
	const opsPerSample = Number(value("ops-per-sample") ?? 100);
	const entryCounts = (value("entry-counts") ?? "0,1,32,100,1000")
		.split(",")
		.map((n) => Number(n.trim()))
		.filter((n) => Number.isFinite(n) && n >= 0);
	const modes = (value("modes") ?? "plain,withFileTypes")
		.split(",")
		.map((mode) => mode.trim())
		.filter(
			(mode): mode is ReaddirMode =>
				mode === "plain" || mode === "withFileTypes",
		);
	if (
		iterations < 1 ||
		warmup < 0 ||
		opsPerSample < 1 ||
		entryCounts.length === 0 ||
		modes.length === 0
	) {
		throw new Error(
			"invalid args; expected --iterations>=1 --warmup>=0 --ops-per-sample>=1 --entry-counts=0,1,32,100,1000 --modes=plain,withFileTypes",
		);
	}
	return { iterations, warmup, entryCounts, modes, opsPerSample };
}

function nowMs(start: number): number {
	return performance.now() - start;
}

function fileName(prefix: string, index: number): string {
	return `${prefix}-${String(index).padStart(5, "0")}.txt`;
}

function names(prefix: string, count: number): string[] {
	return Array.from({ length: count }, (_, index) => fileName(prefix, index));
}

function snapshotForNames(entryNames: string[]) {
	return createSnapshotExport([
		{ path: "/", type: "directory", mode: "0755", uid: 0, gid: 0 },
		{ path: "/bench", type: "directory", mode: "0755", uid: 0, gid: 0 },
		...entryNames.map((name) => ({
			path: `/bench/${name}`,
			type: "file" as const,
			mode: "0644",
			uid: 0,
			gid: 0,
			content: "x",
		})),
	]);
}

async function importSnapshot(
	store: LayerStore,
	entryNames: string[],
): Promise<SnapshotLayerHandle> {
	return store.importSnapshot(snapshotForNames(entryNames));
}

async function createRawMemoryFixture(entryCount: number): Promise<CaseFixture> {
	const filesystem = createInMemoryFileSystem();
	const expectedNames = names("base", entryCount);
	await filesystem.mkdir("/bench", { recursive: true });
	for (const name of expectedNames) {
		await filesystem.writeFile(`/bench/${name}`, "x");
	}
	return {
		filesystem,
		expectedNames,
		dispose() {},
	};
}

async function createCleanOneLowerFixture(
	entryCount: number,
): Promise<CaseFixture> {
	const store = createInMemoryLayerStore();
	const expectedNames = names("base", entryCount);
	const lower = await importSnapshot(store, expectedNames);
	return {
		filesystem: store.createOverlayFilesystem({
			mode: "read-only",
			lowers: [lower],
		}),
		expectedNames,
		dispose: () => store.dispose(),
	};
}

async function createCleanTwoLowerFixture(
	entryCount: number,
): Promise<CaseFixture> {
	const store = createInMemoryLayerStore();
	const expectedNames = names("base", entryCount);
	const highNames = expectedNames.filter((_, index) => index % 2 === 0);
	const lowNames = expectedNames.filter((_, index) => index % 2 === 1);
	const high = await importSnapshot(store, highNames);
	const low = await importSnapshot(store, lowNames);
	return {
		filesystem: store.createOverlayFilesystem({
			mode: "read-only",
			lowers: [high, low],
		}),
		expectedNames,
		dispose: () => store.dispose(),
	};
}

async function createEmptyUpperFixture(entryCount: number): Promise<CaseFixture> {
	const store = createInMemoryLayerStore();
	const expectedNames = names("base", entryCount);
	const upper = await store.createWritableLayer();
	const lower = await importSnapshot(store, expectedNames);
	return {
		filesystem: store.createOverlayFilesystem({
			upper,
			lowers: [lower],
		}),
		expectedNames,
		dispose: () => store.dispose(),
	};
}

async function createUpperMergeFixture(entryCount: number): Promise<CaseFixture> {
	const fixture = await createEmptyUpperFixture(entryCount);
	const upperNames = names("upper", entryCount);
	for (const name of upperNames) {
		await fixture.filesystem.writeFile(`/bench/${name}`, "x");
	}
	return {
		...fixture,
		expectedNames: [...fixture.expectedNames, ...upperNames],
	};
}

async function createUpperShadowFixture(entryCount: number): Promise<CaseFixture> {
	const fixture = await createEmptyUpperFixture(entryCount);
	for (const name of fixture.expectedNames) {
		await fixture.filesystem.writeFile(`/bench/${name}`, "upper");
	}
	return fixture;
}

async function createWhiteoutFixture(entryCount: number): Promise<CaseFixture> {
	const fixture = await createEmptyUpperFixture(entryCount);
	const removed = new Set<string>();
	for (let index = 0; index < fixture.expectedNames.length; index++) {
		const name = fixture.expectedNames[index];
		if (index % 2 === 0) {
			await fixture.filesystem.removeFile(`/bench/${name}`);
			removed.add(name);
		}
	}
	return {
		...fixture,
		expectedNames: fixture.expectedNames.filter((name) => !removed.has(name)),
	};
}

async function createOpaqueDirFixture(entryCount: number): Promise<CaseFixture> {
	const fixture = await createEmptyUpperFixture(entryCount);
	await fixture.filesystem.chmod("/bench", 0o755);
	const upperNames = names("opaque", entryCount);
	for (const name of upperNames) {
		await fixture.filesystem.writeFile(`/bench/${name}`, "x");
	}
	return {
		...fixture,
		expectedNames: upperNames,
	};
}

const CASES: {
	name: string;
	create(entryCount: number): Promise<CaseFixture>;
}[] = [
	{ name: "raw-memory", create: createRawMemoryFixture },
	{ name: "overlay-clean-one-lower", create: createCleanOneLowerFixture },
	{ name: "overlay-clean-two-lower", create: createCleanTwoLowerFixture },
	{ name: "overlay-empty-upper", create: createEmptyUpperFixture },
	{ name: "overlay-upper-merge", create: createUpperMergeFixture },
	{ name: "overlay-upper-shadows", create: createUpperShadowFixture },
	{ name: "overlay-whiteouts-half", create: createWhiteoutFixture },
	{ name: "overlay-opaque-dir", create: createOpaqueDirFixture },
];

function normalizePayload(payload: string[] | VirtualDirEntry[]): string[] {
	return payload.map((entry) => (typeof entry === "string" ? entry : entry.name));
}

async function readPayload(
	filesystem: VirtualFileSystem,
	mode: ReaddirMode,
): Promise<string[] | VirtualDirEntry[]> {
	return mode === "withFileTypes"
		? filesystem.readDirWithTypes("/bench")
		: filesystem.readDir("/bench");
}

function verifyNames(
	scenario: string,
	mode: ReaddirMode,
	payload: string[] | VirtualDirEntry[],
	expectedNames: string[],
): void {
	const actual = normalizePayload(payload).sort();
	const expected = [...expectedNames].sort();
	if (
		actual.length !== expected.length ||
		actual.some((name, index) => name !== expected[index])
	) {
		throw new Error(
			`${scenario} ${mode} returned ${actual.length} entries, expected ${expected.length}`,
		);
	}
}

function payloadBytes(payload: unknown): number {
	return Buffer.byteLength(JSON.stringify(payload));
}

async function runCase(
	scenario: string,
	entryCount: number,
	mode: ReaddirMode,
	iterations: number,
	warmup: number,
	opsPerSample: number,
	create: (entryCount: number) => Promise<CaseFixture>,
): Promise<OverlayCaseResult> {
	const fixture = await create(entryCount);
	try {
		const firstPayload = await readPayload(fixture.filesystem, mode);
		verifyNames(scenario, mode, firstPayload, fixture.expectedNames);

		const rawPerOpMs: number[] = [];
		const rawBatchMs: number[] = [];
		let returnedCount = firstPayload.length;
		let bytes = payloadBytes(firstPayload);

		for (let i = 0; i < warmup + iterations; i++) {
			const start = performance.now();
			let payload: string[] | VirtualDirEntry[] = [];
			for (let op = 0; op < opsPerSample; op++) {
				payload = await readPayload(fixture.filesystem, mode);
			}
			const batchMs = nowMs(start);
			returnedCount = payload.length;
			if (returnedCount !== fixture.expectedNames.length) {
				throw new Error(
					`${scenario} ${mode} returned ${returnedCount}, expected ${fixture.expectedNames.length}`,
				);
			}
			if (i >= warmup) {
				rawBatchMs.push(batchMs);
				rawPerOpMs.push(batchMs / opsPerSample);
				bytes = payloadBytes(payload);
			}
		}

		const readdir = stats(rawPerOpMs);
		return {
			scenario,
			entryCount,
			mode,
			iterations,
			warmup,
			opsPerSample,
			expectedCount: fixture.expectedNames.length,
			returnedCount,
			payloadBytes: bytes,
			readdir,
			sampleBatchMs: stats(rawBatchMs),
			msPerEntry:
				fixture.expectedNames.length === 0
					? readdir.p50
					: round(readdir.p50 / fixture.expectedNames.length, 5),
			rawPerOpMs: rawPerOpMs.map((value) => round(value, 5)),
			rawBatchMs: rawBatchMs.map((value) => round(value, 5)),
		};
	} finally {
		fixture.dispose();
	}
}

async function main(): Promise<void> {
	const { iterations, warmup, entryCounts, modes, opsPerSample } = parseArgs();
	const hardware = getHardware();
	console.error("=== Overlay Readdir Benchmark ===");
	console.error(`CPU: ${hardware.cpu}`);
	console.error(`RAM: ${hardware.ram} | Node: ${hardware.node}`);
	console.error(
		`Iterations: ${iterations} (+ ${warmup} warmup), ops/sample: ${opsPerSample}, entry counts: ${entryCounts.join(",")}, modes: ${modes.join(",")}`,
	);

	const cases: OverlayCaseResult[] = [];
	for (const entryCount of entryCounts) {
		for (const mode of modes) {
			for (const benchCase of CASES) {
				cases.push(
					await runCase(
						benchCase.name,
						entryCount,
						mode,
						iterations,
						warmup,
						opsPerSample,
						benchCase.create,
					),
				);
			}
		}
	}

	printTable(
		["scenario", "mode", "entries", "returned", "p50/op", "p95/op", "ms/entry"],
		cases.map((result) => [
			result.scenario,
			result.mode,
			result.entryCount,
			result.returnedCount,
			`${result.readdir.p50}ms`,
			`${result.readdir.p95}ms`,
			`${result.msPerEntry}ms`,
		]),
	);

	console.log(
		JSON.stringify(
			{
				benchmark: "overlay-readdir",
				hardware,
				iterations,
				warmup,
				opsPerSample,
				entryCounts,
				modes,
				cases,
			},
			null,
			2,
		),
	);
}

main().catch((error) => {
	console.error(error);
	process.exit(1);
});
