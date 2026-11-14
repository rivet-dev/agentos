import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { pssBytes, sidecarPid } from "./helpers/sidecar-memory.js";

type Sidecar = Awaited<ReturnType<typeof AgentOs.createSidecar>>;

const MiB = 1024 * 1024;
const LIVE_WORKERS = 6;
// A VM with an idle Node child costs about 17 MiB of sidecar memory, most of
// it the child's V8 heap. Isolates that each keep a private copy of the V8
// snapshot (7 MiB) or of the bridge source (2.5 MiB per holder) pass 20 MiB.
const MAX_BYTES_PER_LIVE_WORKER = 20 * MiB;
// The guest drops about 20 MiB of strings before it goes idle: enough to
// measure, too little to start a V8 major GC by itself. Nothing allocates
// afterwards, so only an idle-time GC can return that heap.
const GUEST_GARBAGE_STRINGS = 100_000;
const MIN_IDLE_HEAP_RELEASE_BYTES = 10 * MiB;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Worker {
	vm: AgentOs;
	pid: number;
	output: () => string;
}

/** A VM with a Node child that has printed `ready` and then waits. */
async function startWorker(sidecar: Sidecar, code: string): Promise<Worker> {
	const vm = await AgentOs.create({
		sidecar: { kind: "explicit", handle: sidecar },
	});
	let output = "";
	let ready!: () => void;
	const readyPromise = new Promise<void>((resolve) => {
		ready = resolve;
	});
	const { pid } = await vm.process.spawn("node", ["-e", code], {
		onStdout: (chunk) => {
			output += Buffer.from(chunk).toString();
			if (output.includes("ready")) ready();
		},
	});
	await readyPromise;
	return { vm, pid, output: () => output };
}

async function stopWorker(worker: Worker) {
	await worker.vm.process.kill(worker.pid);
	await worker.vm.process.wait(worker.pid);
	await worker.vm.dispose();
}

const IDLE_CHILD = "process.stdout.write('ready'); setInterval(() => {}, 1e6);";

test.skipIf(process.platform !== "linux")(
	"live VMs with idle Node children share the read-only runtime data",
	async () => {
		const sidecar = await AgentOs.createSidecar();
		const workers: Worker[] = [];
		try {
			workers.push(await startWorker(sidecar, IDLE_CHILD));
			const pid = sidecarPid(workers[0].vm);
			await sleep(1_500);
			const baseline = pssBytes(pid);

			for (let i = 0; i < LIVE_WORKERS; i++) {
				workers.push(await startWorker(sidecar, IDLE_CHILD));
			}
			await sleep(2_000);
			const perWorker = (pssBytes(pid) - baseline) / LIVE_WORKERS;

			expect(perWorker).toBeLessThan(MAX_BYTES_PER_LIVE_WORKER);
		} finally {
			for (const worker of workers) await stopWorker(worker);
			await sidecar.dispose();
		}
	},
	120_000,
);

test.skipIf(process.platform !== "linux")(
	"an idle Node child gives back the heap its startup work left behind",
	async () => {
		const sidecar = await AgentOs.createSidecar();
		const report = "JSON.stringify(process.memoryUsage().heapUsed)";
		const worker = await startWorker(
			sidecar,
			`let garbage = Array.from({ length: ${GUEST_GARBAGE_STRINGS} }, (_, i) => "x".repeat(80) + i);
			process.stdout.write("busy=" + ${report} + "\\n");
			garbage = null;
			process.stdout.write("ready\\n");
			setTimeout(() => process.stdout.write("idle=" + ${report} + "\\n"), 3_000);
			setInterval(() => {}, 1e6);`,
		);
		try {
			const deadline = Date.now() + 10_000;
			while (!worker.output().includes("idle=") && Date.now() < deadline) {
				await sleep(100);
			}
			const busy = Number(worker.output().match(/busy=(\d+)/)?.[1]);
			const idle = Number(worker.output().match(/idle=(\d+)/)?.[1]);
			expect(Number.isFinite(idle)).toBe(true);

			expect(busy - idle).toBeGreaterThan(MIN_IDLE_HEAP_RELEASE_BYTES);
		} finally {
			await stopWorker(worker);
			await sidecar.dispose();
		}
	},
	60_000,
);
