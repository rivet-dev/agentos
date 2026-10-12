import type { ChildProcess } from "node:child_process";
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { AgentOs } from "../src/index.js";
import type { StdioSidecarProcess } from "../src/process.js";
import { bodyWithCleanup } from "./helpers/native-owner-fixture.js";

const FILE_BYTES = 8 * 1024 * 1024;
const WARMUP_CYCLES = 3;
const MEASURED_CYCLES = 12;
// Less than the content of two disposed VMs. A sidecar that keeps each
// disposed VM's filesystem grows by more than FILE_BYTES per cycle.
const MAX_GROWTH_BYTES = 2 * FILE_BYTES;

// Reaches into internals only to find the sidecar process; the behavior under
// test is the memory the OS reports for it.
function sidecarChild(vm: AgentOs): ChildProcess {
	return (
		vm as unknown as {
			_sidecarClient: {
				protocolClient: { sidecarProcess: StdioSidecarProcess };
			};
		}
	)._sidecarClient.protocolClient.sidecarProcess.child;
}

function pssBytes(pid: number): number {
	const rollup = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
	const match = rollup.match(/^Pss:\s+(\d+) kB$/m);
	if (!match) throw new Error(`no Pss line in /proc/${pid}/smaps_rollup`);
	return Number(match[1]) * 1024;
}

async function cycle(
	sidecar: Awaited<ReturnType<typeof AgentOs.createSidecar>>,
) {
	const vm = await AgentOs.create({
		defaultSoftware: false,
		sidecar: { kind: "explicit", handle: sidecar },
	});
	await bodyWithCleanup(
		() => vm.writeFile("/tmp/payload.bin", new Uint8Array(FILE_BYTES).fill(7)),
		() => vm.dispose(),
	);
}

// Observe the same bounded settling period after disposal; no allocator action is assumed.
const settle = () => new Promise((resolve) => setTimeout(resolve, 1_500));

test.skipIf(process.platform !== "linux")(
	"disposed VMs return their filesystem memory to the sidecar",
	async () => {
		const sidecar = await AgentOs.createSidecar();
		let keep: AgentOs | undefined;
		await bodyWithCleanup(
			async () => {
				keep = await AgentOs.create({
					defaultSoftware: false,
					sidecar: { kind: "explicit", handle: sidecar },
				});
				const pid = sidecarChild(keep).pid;
				if (pid === undefined) throw new Error("sidecar process has no pid");

				for (let i = 0; i < WARMUP_CYCLES; i++) await cycle(sidecar);
				await settle();
				const baseline = pssBytes(pid);

				for (let i = 0; i < MEASURED_CYCLES; i++) await cycle(sidecar);
				await settle();
				const growth = pssBytes(pid) - baseline;

				expect(growth).toBeLessThan(MAX_GROWTH_BYTES);
			},
			async () => {
				// Attempt both independent owners even if keep disposal fails.
				await bodyWithCleanup(
					async () => {
						await keep?.dispose();
					},
					() => sidecar.dispose(),
				);
			},
		);
	},
	120_000,
);
