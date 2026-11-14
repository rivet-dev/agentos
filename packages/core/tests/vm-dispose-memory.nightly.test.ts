import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import { pssBytes, sidecarPid } from "./helpers/sidecar-memory.js";

const FILE_BYTES = 8 * 1024 * 1024;
const WARMUP_CYCLES = 3;
const MEASURED_CYCLES = 12;
// Less than the content of two disposed VMs. A sidecar that keeps each
// disposed VM's filesystem grows by more than FILE_BYTES per cycle.
const MAX_GROWTH_BYTES = 2 * FILE_BYTES;

async function cycle(sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>) {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		sidecar: { kind: "explicit", handle: sidecar },
	});
	try {
		await vm.writeFile("/tmp/payload.bin", new Uint8Array(FILE_BYTES).fill(7));
	} finally {
		await vm.dispose();
	}
}

// The sidecar trims the native heap about 1 s after a VM disposal.
const settle = () => new Promise((resolve) => setTimeout(resolve, 1_500));

test.skipIf(process.platform !== "linux")(
	"disposed VMs return their filesystem memory to the sidecar",
	async () => {
		const sidecar = await AgentOs.createSidecar();
		const keep = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
		});
		try {
			const pid = sidecarPid(keep);

			for (let i = 0; i < WARMUP_CYCLES; i++) await cycle(sidecar);
			await settle();
			const baseline = pssBytes(pid);

			for (let i = 0; i < MEASURED_CYCLES; i++) await cycle(sidecar);
			await settle();
			const growth = pssBytes(pid) - baseline;

			expect(growth).toBeLessThan(MAX_GROWTH_BYTES);
		} finally {
			await keep.dispose();
			await sidecar.dispose();
		}
	},
	120_000,
);
