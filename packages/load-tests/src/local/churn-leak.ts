import { AgentOs, type AgentOsSidecar } from "@rivet-dev/agentos-core";
import {
	cgroupSnapshot,
	errorText,
	forceGc,
	integerEnv,
	linearSlope,
	newRunId,
	numberEnv,
	percentile,
	runtimeProvenance,
	sampleProcessTree,
	sleep,
	type ProcessTreeSample,
	type TimedProbe,
	withTimeout,
	writeArtifact,
} from "../common.js";

interface ChurnSample {
	shape: "sequential" | "burst" | "steady" | "final";
	cycle: number;
	activeVmCount: number;
	processTree: ProcessTreeSample;
}

async function createVm(sidecar: AgentOsSidecar): Promise<AgentOs> {
	return AgentOs.create({
		defaultSoftware: false,
		sidecar: { kind: "explicit", handle: sidecar },
	});
}

async function cleanWork(vm: AgentOs, marker: string): Promise<void> {
	const result = await withTimeout(
		`clean workload ${marker}`,
		vm.execArgv(
			"node",
			["-e", `process.stdout.write(${JSON.stringify(marker)})`],
			{ timeout: 5_000 },
		),
		7_000,
	);
	if (result.exitCode !== 0 || result.stdout !== marker) {
		throw new Error(
			`workload ${marker} failed exit=${result.exitCode} stdout=${JSON.stringify(result.stdout)} stderr=${JSON.stringify(result.stderr)}`,
		);
	}
}

function startDirtyWork(vm: AgentOs): void {
	vm.spawn(
		"node",
		[
			"-e",
			"const net=require('node:net'); const s=net.createServer(()=>{}); s.listen(0,'127.0.0.1'); setInterval(()=>{},1000)",
		],
		{ captureStdio: false },
	);
}

async function sentinelProbe(vm: AgentOs): Promise<TimedProbe> {
	const startedAtMs = Date.now();
	try {
		await cleanWork(vm, "sentinel");
		return { startedAtMs, durationMs: Date.now() - startedAtMs, ok: true };
	} catch (error) {
		return {
			startedAtMs,
			durationMs: Date.now() - startedAtMs,
			ok: false,
			error: errorText(error),
		};
	}
}

async function settledSample(): Promise<ProcessTreeSample> {
	forceGc();
	forceGc();
	await sleep(100);
	return sampleProcessTree();
}

export async function runChurnLeak(): Promise<void> {
	const runId = newRunId("churn");
	// Default high enough that the post-warmup slope window clears V8/allocator
	// warmup (which asymptotes over the first ~12 lifecycles on this host).
	const cycles = integerEnv("LOAD_TEST_CYCLES", 20);
	const batchSize = integerEnv("LOAD_TEST_BATCH", 4);
	const settleMs = integerEnv("LOAD_TEST_SETTLE_MS", 500);
	// Plateau slope gate (bytes/sample). A true leak keeps climbing once warm; V8
	// arena retention flattens. Applies to the constant-live steady phase and the
	// zero-attacker post-teardown series.
	const rssSlopeLimit = numberEnv("LOAD_TEST_RSS_SLOPE_BYTES", 1024 * 1024);
	// Total-growth ceilings are PROVISIONAL and calibrated from >=5 clean reps on
	// the canonical host (see docs-internal/load-testing.md). RSS is generous
	// because glibc/V8 retain freed pages; PSS is the leak-accurate signal.
	const rssTotalLimit = numberEnv("LOAD_TEST_RSS_TOTAL_BYTES", 256 * 1024 * 1024);
	const pssTotalLimit = numberEnv("LOAD_TEST_PSS_TOTAL_BYTES", 160 * 1024 * 1024);
	const failures: string[] = [];
	const operationErrors: string[] = [];
	const samples: ChurnSample[] = [];
	const probes: TimedProbe[] = [];
	let sidecar: AgentOsSidecar | undefined;
	let sentinel: AgentOs | undefined;
	let baseline: ProcessTreeSample | undefined;
	let final: ProcessTreeSample | undefined;
	const cgroupBefore = cgroupSnapshot();

	try {
		sidecar = await AgentOs.createSidecar({ sidecarId: `load-churn-${runId}` });
		sentinel = await createVm(sidecar);

		// Warm process-global V8, protocol, and allocator paths before the baseline.
		for (let index = 0; index < 2; index += 1) {
			const vm = await createVm(sidecar);
			await cleanWork(vm, `warmup-${index}`);
			await vm.dispose();
		}
		for (let index = 0; index < 5; index += 1) {
			probes.push(await sentinelProbe(sentinel));
		}
		await sleep(settleMs);
		baseline = await settledSample();

		for (let cycle = 0; cycle < cycles; cycle += 1) {
			try {
				const vm = await createVm(sidecar);
				if (cycle % 3 === 2) startDirtyWork(vm);
				else await cleanWork(vm, `sequential-${cycle}`);
				await vm.dispose();
			} catch (error) {
				operationErrors.push(`sequential ${cycle}: ${errorText(error)}`);
			}
			probes.push(await sentinelProbe(sentinel));
			samples.push({
				shape: "sequential",
				cycle,
				activeVmCount: sidecar.describe().activeVmCount,
				processTree: await settledSample(),
			});
		}

		for (let cycle = 0; cycle < cycles; cycle += 1) {
			let vms: AgentOs[] = [];
			try {
				vms = await Promise.all(
					Array.from({ length: batchSize }, () => createVm(sidecar!)),
				);
				await Promise.all(
					vms.map((vm, index) =>
						index % 2 === 0
							? cleanWork(vm, `burst-${cycle}-${index}`)
							: Promise.resolve(startDirtyWork(vm)),
					),
				);
			} catch (error) {
				operationErrors.push(`burst ${cycle}: ${errorText(error)}`);
			} finally {
				await Promise.all(vms.map((vm) => vm.dispose().catch(() => {})));
			}
			probes.push(await sentinelProbe(sentinel));
			samples.push({
				shape: "burst",
				cycle,
				activeVmCount: sidecar.describe().activeVmCount,
				processTree: await settledSample(),
			});
		}

		let steady: AgentOs[] = [];
		try {
			steady = await Promise.all(
				Array.from({ length: batchSize }, () => createVm(sidecar!)),
			);
			for (let cycle = 0; cycle < cycles; cycle += 1) {
				const slot = cycle % batchSize;
				await steady[slot]!.dispose();
				const replacement = await createVm(sidecar);
				if (cycle % 2 === 0) await cleanWork(replacement, `steady-${cycle}`);
				else startDirtyWork(replacement);
				steady[slot] = replacement;
				probes.push(await sentinelProbe(sentinel));
				samples.push({
					shape: "steady",
					cycle,
					activeVmCount: sidecar.describe().activeVmCount,
					processTree: await settledSample(),
				});
			}
		} catch (error) {
			operationErrors.push(`steady: ${errorText(error)}`);
		} finally {
			await Promise.all(steady.map((vm) => vm.dispose().catch(() => {})));
		}

		await sleep(settleMs);
		for (let index = 0; index < 5; index += 1) {
			probes.push(await sentinelProbe(sentinel));
		}
		final = await settledSample();
		samples.push({
			shape: "final",
			cycle: cycles,
			activeVmCount: sidecar.describe().activeVmCount,
			processTree: final,
		});
	} catch (error) {
		operationErrors.push(`top-level: ${errorText(error)}`);
	} finally {
		if (sentinel) await sentinel.dispose().catch(() => {});
		if (sidecar) await sidecar.dispose().catch(() => {});
	}

	await sleep(250);
	// Zero-attacker post-teardown plateau: with nothing live, RSS/PSS must stop
	// trending up. Fitting the slope here (not on the warming sequential phase)
	// is what actually distinguishes a leak from V8/glibc arena retention.
	const teardownSeries: ProcessTreeSample[] = [];
	for (let index = 0; index < 8; index += 1) {
		teardownSeries.push(await settledSample());
		await sleep(200);
	}
	const afterDispose = teardownSeries[teardownSeries.length - 1]!;

	// Constant-live steady-state samples: a leak hidden by always-live VMs shows
	// here as a persistent positive per-cycle slope even though the live-VM count
	// is fixed. Fit over the POST-WARMUP window (last third) so V8/glibc arena
	// warmup — which asymptotes and whose average slope shrinks as the run
	// lengthens (a true leak's slope would not) — does not trip the gate.
	const steadyRss = samples
		.filter((sample) => sample.shape === "steady")
		.map((sample) => sample.processTree.rssBytes);
	const steadyPostWarmup = steadyRss.slice(Math.floor((steadyRss.length * 2) / 3));
	const steadySlopeBytesPerCycle = linearSlope(steadyPostWarmup);
	const teardownRssSlope = linearSlope(teardownSeries.map((s) => s.rssBytes));
	const teardownPssValues = teardownSeries.map((s) => s.pssBytes).filter((v) => v >= 0);
	const teardownPssSlope =
		teardownPssValues.length >= 2 ? linearSlope(teardownPssValues) : 0;

	const rssTotalGrowthBytes =
		baseline && final ? final.rssBytes - baseline.rssBytes : Number.POSITIVE_INFINITY;
	const pssTotalGrowthBytes =
		baseline && final && baseline.pssBytes >= 0 && final.pssBytes >= 0
			? final.pssBytes - baseline.pssBytes
			: null;
	const probeFailures = probes.filter((probe) => !probe.ok);
	const baselineProbeP99 = percentile(
		probes.slice(0, 5).filter((probe) => probe.ok).map((probe) => probe.durationMs),
		0.99,
	);
	const finalProbeP99 = percentile(
		probes.slice(-5).filter((probe) => probe.ok).map((probe) => probe.durationMs),
		0.99,
	);

	if (operationErrors.length > 0) {
		failures.push(`${operationErrors.length} lifecycle operation(s) failed`);
	}
	if (!baseline || !final) failures.push("missing baseline or final process census");
	// Plateau slopes are the primary memory-leak gate.
	if (steadySlopeBytesPerCycle > rssSlopeLimit) {
		failures.push(
			`steady-state RSS slope ${Math.round(steadySlopeBytesPerCycle)} B/cycle exceeds ${rssSlopeLimit}`,
		);
	}
	if (teardownRssSlope > rssSlopeLimit) {
		failures.push(
			`post-teardown RSS slope ${Math.round(teardownRssSlope)} B/sample exceeds ${rssSlopeLimit}`,
		);
	}
	if (teardownPssSlope > rssSlopeLimit) {
		failures.push(
			`post-teardown PSS slope ${Math.round(teardownPssSlope)} B/sample exceeds ${rssSlopeLimit}`,
		);
	}
	// Total-growth ceilings are provisional/calibrated; PSS is the tighter signal.
	if (rssTotalGrowthBytes > rssTotalLimit) {
		failures.push(`RSS growth ${rssTotalGrowthBytes} B exceeds ${rssTotalLimit}`);
	}
	if (pssTotalGrowthBytes !== null && pssTotalGrowthBytes > pssTotalLimit) {
		failures.push(`PSS growth ${pssTotalGrowthBytes} B exceeds ${pssTotalLimit}`);
	}
	if (baseline && final && final.fdCount > baseline.fdCount) {
		failures.push(`fd count grew from ${baseline.fdCount} to ${final.fdCount}`);
	}
	if (baseline && final && final.threadCount > baseline.threadCount) {
		failures.push(
			`thread count grew from ${baseline.threadCount} to ${final.threadCount}`,
		);
	}
	if (baseline && final && final.pidCount > baseline.pidCount) {
		failures.push(`process count grew from ${baseline.pidCount} to ${final.pidCount}`);
	}
	if (probeFailures.length > 0) failures.push(`${probeFailures.length} sentinel probes failed`);
	if (baselineProbeP99 > 0 && finalProbeP99 > baselineProbeP99 * 2) {
		failures.push(
			`post-pressure sentinel p99 ${finalProbeP99}ms exceeds 2x baseline ${baselineProbeP99}ms`,
		);
	}
	for (const sample of samples) {
		const expected = sample.shape === "steady" ? batchSize + 1 : 1;
		if (sample.activeVmCount !== expected) {
			failures.push(
				`${sample.shape} cycle ${sample.cycle} retained ${sample.activeVmCount} VMs; expected ${expected}`,
			);
			break;
		}
	}
	const cgroupAfter = cgroupSnapshot();
	const oomKillsBefore = Number(cgroupBefore["memory.events.oom_kill"] ?? 0);
	const oomKillsAfter = Number(cgroupAfter["memory.events.oom_kill"] ?? 0);
	if (oomKillsAfter > oomKillsBefore) {
		failures.push(`container recorded ${oomKillsAfter - oomKillsBefore} OOM kill(s)`);
	}

	const artifact = {
		runId,
		lane: "vm-lifecycle-churn-leak",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		operationErrors,
		config: {
			cyclesPerShape: cycles,
			batchSize,
			settleMs,
			rssSlopeLimit,
			rssTotalLimit,
			pssTotalLimit,
		},
		provenance: runtimeProvenance(),
		metrics: {
			steadySlopeBytesPerCycle,
			teardownRssSlope,
			teardownPssSlope,
			rssTotalGrowthBytes,
			pssTotalGrowthBytes,
			baselineProbeP99,
			finalProbeP99,
			probeFailures: probeFailures.length,
		},
		baseline,
		final,
		afterDispose,
		teardownSeries,
		samples,
		probes,
		cgroupBefore,
		cgroupAfter,
	};
	const path = writeArtifact("churn", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, metrics: artifact.metrics }));
	if (failures.length > 0) process.exitCode = 1;
}
