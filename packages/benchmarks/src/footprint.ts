import { readRssBytes } from "./lib/memory.js";
import { writeJson } from "./lib/report.js";
import { type BenchVm, createBenchSidecar, createBenchVm } from "./lib/vm.js";

const RESULTS_DIR = new URL("../results/", import.meta.url).pathname;

export async function runFootprint() {
	const sidecar = createBenchSidecar();
	let vm: BenchVm | null = null;
	try {
		vm = await createBenchVm({ sidecar });
		await new Promise((resolve) => setTimeout(resolve, 250));
		const measuredPid = vm.sidecarPid();
		const out = {
			idleRssFloorBytes: readRssBytes(measuredPid),
			measuredPid,
			resource: await vm.getResourceSnapshot(),
			components: [],
			findings: [],
		};
		writeJson(`${RESULTS_DIR}/footprint.json`, out);
		return out;
	} finally {
		await vm?.dispose();
		await sidecar.dispose();
	}
}

if (import.meta.url === `file://${process.argv[1]}`) {
	const out = await runFootprint();
	console.log(JSON.stringify(out, null, 2));
	process.exit(0);
}
