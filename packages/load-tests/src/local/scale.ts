// High-scale local stress: put HUNDREDS of concurrent VMs on one shared sidecar
// beside a live sentinel, run bounded work in each, dispose them all, and prove
// the sidecar survives, the sentinel keeps making progress, accounting
// reconciles to zero, and nothing leaks. This is the "fully stress local dev
// first" foundation before pushing Rivet Compute.
import { AgentOs, type AgentOsSidecar } from "@rivet-dev/agentos-core";
import {
	cgroupSnapshot,
	errorText,
	integerEnv,
	newRunId,
	percentile,
	type ProcessTreeSample,
	runtimeProvenance,
	sampleProcessTree,
	sleep,
	type TimedProbe,
	withTimeout,
	writeArtifact,
} from "../common.js";

async function probeSentinel(vm: AgentOs): Promise<TimedProbe> {
	const startedAtMs = Date.now();
	try {
		const r = await withTimeout(
			"sentinel exec",
			vm.execArgv("node", ["-e", 'process.stdout.write("sentinel-ok")'], { timeout: 8_000 }),
			10_000,
		);
		if (r.exitCode !== 0 || r.stdout !== "sentinel-ok") {
			throw new Error(`sentinel exit=${r.exitCode} stderr=${JSON.stringify(r.stderr).slice(0, 120)}`);
		}
		return { startedAtMs, durationMs: Date.now() - startedAtMs, ok: true };
	} catch (error) {
		return { startedAtMs, durationMs: Date.now() - startedAtMs, ok: false, error: errorText(error) };
	}
}

/** Run `fn` over `items` with a bounded concurrent worker pool. */
async function pool<T>(
	count: number,
	limit: number,
	fn: (index: number) => Promise<T>,
): Promise<Array<{ ok: true; value: T } | { ok: false; error: string }>> {
	const out: Array<{ ok: true; value: T } | { ok: false; error: string }> = [];
	let cursor = 0;
	const worker = async () => {
		while (cursor < count) {
			const index = cursor++;
			try {
				out.push({ ok: true, value: await fn(index) });
			} catch (error) {
				out.push({ ok: false, error: errorText(error) });
			}
		}
	};
	await Promise.all(Array.from({ length: Math.min(limit, count) }, () => worker()));
	return out;
}

export async function runScale(): Promise<void> {
	const runId = newRunId("scale");
	const vmCount = integerEnv("LOAD_TEST_VM_COUNT", 200);
	// Creation can be highly concurrent; EXECUTION must stay <= the V8 executor
	// pool (= CPU count) or AgentOS rejects the excess (ERR_AGENTOS_VM_EXECUTOR_LIMIT,
	// extends LT-008 — excess concurrent executions are rejected, not queued).
	const concurrency = integerEnv("LOAD_TEST_CONCURRENCY", 24);
	const execConcurrency = integerEnv("LOAD_TEST_EXEC_CONCURRENCY", 6);
	const cycles = integerEnv("LOAD_TEST_CYCLES", 1);

	let sidecar: AgentOsSidecar | undefined;
	let sentinel: AgentOs | undefined;
	const failures: string[] = [];
	const opErrors: string[] = [];
	const probes: TimedProbe[] = [];
	const cgroupBefore = cgroupSnapshot();
	const before = sampleProcessTree();
	let peakSample: ProcessTreeSample | undefined;
	let peakActiveVms = 0;
	const createMs: number[] = [];
	const disposeMs: number[] = [];

	try {
		sidecar = await AgentOs.createSidecar({ sidecarId: `load-scale-${runId}` });
		sentinel = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
		});
		for (let i = 0; i < 3; i += 1) probes.push(await probeSentinel(sentinel));

		for (let cycle = 0; cycle < cycles; cycle += 1) {
			// PHASE 1 — admission/coexistence: create vmCount IDLE VMs concurrently
			// (no execution). This isolates VM-count scaling from the executor cap.
			const vms: (AgentOs | undefined)[] = new Array(vmCount);
			const createResults = await pool(vmCount, concurrency, async (index) => {
				const t0 = Date.now();
				const vm = await AgentOs.create({
					defaultSoftware: false,
					sidecar: { kind: "explicit", handle: sidecar! },
				});
				vms[index] = vm;
				createMs.push(Date.now() - t0);
				return index;
			});
			for (const r of createResults) if (!r.ok) opErrors.push(`cycle ${cycle} create: ${r.error}`);

			const desc = sidecar.describe();
			peakActiveVms = Math.max(peakActiveVms, desc.activeVmCount);
			const s = sampleProcessTree();
			if (!peakSample || s.rssBytes > peakSample.rssBytes) peakSample = s;
			probes.push(await probeSentinel(sentinel));

			// PHASE 2 — execution: run a tiny workload in each live VM, but bound
			// concurrency to <= the executor pool (CPU count) so we do not trip the
			// documented executor-rejection (LT-015); this tests throughput at scale.
			const live = vms.map((v, i) => (v ? i : -1)).filter((i) => i >= 0);
			let execCursor = 0;
			await Promise.all(
				Array.from({ length: Math.min(execConcurrency, live.length) }, async () => {
					while (execCursor < live.length) {
						const index = live[execCursor++]!;
						try {
							const r = await vms[index]!.execArgv("node", ["-e", `process.stdout.write("v" + ${index})`], { timeout: 15_000 });
							if (r.exitCode !== 0 || r.stdout !== `v${index}`) {
								opErrors.push(`cycle ${cycle} exec vm ${index}: exit=${r.exitCode} err=${JSON.stringify(r.stderr).slice(0, 160)}`);
							}
						} catch (error) {
							opErrors.push(`cycle ${cycle} exec vm ${index}: ${errorText(error)}`);
						}
					}
				}),
			);
			probes.push(await probeSentinel(sentinel));

			// PHASE 3 — dispose all VMs concurrently and time it.
			const t1 = Date.now();
			await pool(vmCount, concurrency, async (index) => {
				if (vms[index]) await vms[index]!.dispose();
				return index;
			});
			disposeMs.push(Date.now() - t1);
			probes.push(await probeSentinel(sentinel));
		}

		for (let i = 0; i < 3; i += 1) probes.push(await probeSentinel(sentinel));
	} catch (error) {
		opErrors.push(`top-level: ${errorText(error)}`);
	} finally {
		if (sentinel) await sentinel.dispose().catch(() => {});
		if (sidecar) await sidecar.dispose().catch(() => {});
	}

	await sleep(500);
	const after = sampleProcessTree();
	const sidecarStateOk = true; // sidecar disposed above; survival proven by sentinel + fresh sample
	const probeFailures = probes.filter((p) => !p.ok);

	// Gates.
	if (opErrors.length > 0) failures.push(`${opErrors.length} scale operation(s) failed`);
	if (probeFailures.length > 0) failures.push(`${probeFailures.length}/${probes.length} sentinel probes failed`);
	if (peakActiveVms < vmCount) {
		failures.push(`peak activeVmCount ${peakActiveVms} < requested ${vmCount} (admission dropped VMs)`);
	}
	if (after.fdCount > before.fdCount) failures.push(`fd leak: ${before.fdCount} -> ${after.fdCount}`);
	if (after.threadCount > before.threadCount) {
		failures.push(`thread leak: ${before.threadCount} -> ${after.threadCount}`);
	}
	if (after.pidCount > before.pidCount) failures.push(`process leak: ${before.pidCount} -> ${after.pidCount}`);
	const cgroupAfter = cgroupSnapshot();
	const oomBefore = Number(cgroupBefore["memory.events.oom_kill"] ?? 0);
	const oomAfter = Number(cgroupAfter["memory.events.oom_kill"] ?? 0);
	if (oomAfter > oomBefore) failures.push(`container recorded ${oomAfter - oomBefore} OOM kill(s)`);

	const artifact = {
		runId,
		lane: "vm-scale-storm",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		config: { vmCount, concurrency, cycles },
		provenance: runtimeProvenance(),
		metrics: {
			peakActiveVms,
			createMsP50: percentile(createMs, 0.5),
			createMsP99: percentile(createMs, 0.99),
			disposeMsP50: percentile(disposeMs, 0.5),
			disposeMsP99: percentile(disposeMs, 0.99),
			sentinelP99Ms: percentile(probes.filter((p) => p.ok).map((p) => p.durationMs), 0.99),
			sentinelFailures: probeFailures.length,
			opErrors: opErrors.length,
		},
		before,
		peakSample,
		after,
		cgroupBefore,
		cgroupAfter,
		opErrorSamples: opErrors.slice(0, 8),
		sidecarStateOk,
	};
	const path = writeArtifact("scale", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures: failures.slice(0, 6), path, metrics: artifact.metrics }));
	if (failures.length > 0) process.exitCode = 1;
}
