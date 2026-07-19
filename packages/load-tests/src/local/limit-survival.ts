import {
	AgentOs,
	type AgentOsSidecar,
	type LimitWarning,
} from "@rivet-dev/agentos-core";
import {
	cgroupSnapshot,
	errorText,
	integerEnv,
	newRunId,
	percentile,
	runtimeProvenance,
	sampleProcessTree,
	sleep,
	type TimedProbe,
	withTimeout,
	writeArtifact,
} from "../common.js";

const PROCESS_LIMIT_PATTERN =
	/EAGAIN|resource temporarily unavailable|maxProcesses|process(?:es)?[^\n]*limit|ERR_AGENTOS/i;

// CommonJS on purpose: AgentOS's `node` runtime does not parse
// `--input-type=module` as a flag (it treats it as the entry module path, see
// LT-006), so the guest workload uses `require` + an async IIFE and is passed
// as a plain `node -e <script>` with no ESM flags.
function guestProcessStorm(attempts: number): string {
	return `
const { spawn } = require("node:child_process");
const attempts = ${attempts};
const children = [];

function start(index) {
  return new Promise((resolve) => {
    let settled = false;
    let child;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    try {
      child = spawn(process.execPath, ["-e", "setTimeout(() => {}, 30000)"], {
        stdio: "ignore",
      });
    } catch (error) {
      finish({ index, status: "error", code: error.code ?? null, message: error.message });
      return;
    }
    children.push(child);
    child.once("spawn", () => finish({ index, status: "spawned", pid: child.pid }));
    child.once("error", (error) => finish({
      index,
      status: "error",
      code: error.code ?? null,
      message: error.message,
    }));
    setTimeout(() => finish({ index, status: "timeout" }), 3000);
  });
}

(async () => {
  const outcomes = await Promise.all(Array.from({ length: attempts }, (_, index) => start(index)));
  for (const child of children) {
    if (child.pid) { try { child.kill("SIGKILL"); } catch {} }
  }
  await new Promise((resolve) => setTimeout(resolve, 100));
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify(outcomes) + "\\n");
})();
`;
}

async function probeSentinel(vm: AgentOs): Promise<TimedProbe> {
	const startedAtMs = Date.now();
	try {
		const result = await withTimeout(
			"sentinel exec",
			vm.execArgv("node", ["-e", 'process.stdout.write("sentinel-ok")'], {
				timeout: 5_000,
			}),
			7_000,
		);
		if (result.exitCode !== 0 || result.stdout !== "sentinel-ok") {
			throw new Error(
				`sentinel returned exit=${result.exitCode} stdout=${JSON.stringify(result.stdout)} stderr=${JSON.stringify(result.stderr)}`,
			);
		}
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

function parseGuestOutcomes(stdout: string): Array<Record<string, unknown>> {
	const line = stdout
		.split("\n")
		.find((candidate) => candidate.startsWith("AGENTOS_LIMIT_RESULT="));
	if (!line) return [];
	const parsed = JSON.parse(line.slice("AGENTOS_LIMIT_RESULT=".length));
	return Array.isArray(parsed) ? parsed : [];
}

export async function runLimitSurvival(): Promise<void> {
	const runId = newRunId("limits");
	const processLimit = integerEnv("LOAD_TEST_PROCESS_LIMIT", 8);
	const attempts = integerEnv("LOAD_TEST_PROCESS_ATTEMPTS", 32);
	if (attempts <= processLimit) {
		throw new Error(
			`LOAD_TEST_PROCESS_ATTEMPTS (${attempts}) must exceed LOAD_TEST_PROCESS_LIMIT (${processLimit})`,
		);
	}

	let sidecar: AgentOsSidecar | undefined;
	let sentinel: AgentOs | undefined;
	let attacker: AgentOs | undefined;
	const warnings: LimitWarning[] = [];
	const probes: TimedProbe[] = [];
	let stopProbing = false;
	let probeLoop: Promise<void> | undefined;
	let topLevelError: string | undefined;
	let attackResult:
		| { exitCode: number; stdout: string; stderr: string }
		| undefined;
	const before = sampleProcessTree();

	try {
		sidecar = await AgentOs.createSidecar({ sidecarId: `load-limits-${runId}` });
		sentinel = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
		});
		attacker = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
			limits: { resources: { maxProcesses: processLimit } },
			onLimitWarning: (warning) => warnings.push(warning),
		});

		for (let index = 0; index < 3; index += 1) {
			probes.push(await probeSentinel(sentinel));
		}
		probeLoop = (async () => {
			while (!stopProbing) {
				probes.push(await probeSentinel(sentinel!));
				await sleep(100);
			}
		})();

		attackResult = await withTimeout(
			"guest process storm",
			attacker.execArgv("node", ["-e", guestProcessStorm(attempts)], {
				timeout: 20_000,
			}),
			25_000,
		);
	} catch (error) {
		topLevelError = errorText(error);
	} finally {
		stopProbing = true;
		await probeLoop;
		if (attacker) await attacker.dispose().catch(() => {});
	}

	if (sentinel) {
		for (let index = 0; index < 3; index += 1) {
			probes.push(await probeSentinel(sentinel));
		}
	}
	await sleep(250);
	const afterAttacker = sampleProcessTree();
	const sidecarAfterAttacker = sidecar?.describe();
	if (sentinel) await sentinel.dispose().catch(() => {});
	if (sidecar) await sidecar.dispose().catch(() => {});
	await sleep(250);
	const final = sampleProcessTree();

	const outcomes = parseGuestOutcomes(attackResult?.stdout ?? "");
	const rejected = outcomes.filter((outcome) => outcome.status === "error");
	const rejectionText = JSON.stringify(rejected);
	const sentinelFailures = probes.filter((probe) => !probe.ok);
	const sentinelP99Ms = percentile(
		probes.filter((probe) => probe.ok).map((probe) => probe.durationMs),
		0.99,
	);
	const failures: string[] = [];
	if (topLevelError) failures.push(`attack threw before returning evidence: ${topLevelError}`);
	if (!attackResult) failures.push("attack produced no execution result");
	if (attackResult && attackResult.exitCode !== 0) {
		failures.push(`attack parent exited ${attackResult.exitCode}`);
	}
	if (rejected.length === 0) failures.push("guest process storm observed no rejected spawn");
	if (rejected.length > 0 && !PROCESS_LIMIT_PATTERN.test(rejectionText)) {
		failures.push(`spawn rejection did not identify a process limit: ${rejectionText}`);
	}
	if (warnings.length === 0) failures.push("host received no near-limit warning");
	if (sentinelFailures.length > 0) {
		failures.push(`${sentinelFailures.length} sentinel probes failed`);
	}
	if (sidecarAfterAttacker?.state !== "ready") {
		failures.push(`sidecar state after attack was ${sidecarAfterAttacker?.state}`);
	}
	if (sidecarAfterAttacker?.activeVmCount !== 1) {
		failures.push(
			`expected only the sentinel VM after attacker disposal, observed ${sidecarAfterAttacker?.activeVmCount}`,
		);
	}
	const oomKills = Number(cgroupSnapshot()["memory.events.oom_kill"] ?? 0);
	if (oomKills > 0) failures.push(`container recorded ${oomKills} OOM kill(s)`);

	const artifact = {
		runId,
		lane: "guest-limit-host-survival",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		config: { processLimit, attempts },
		provenance: runtimeProvenance(),
		attack: {
			exitCode: attackResult?.exitCode,
			stderr: attackResult?.stderr,
			outcomes,
			rejectedCount: rejected.length,
			topLevelError,
		},
		warnings,
		sentinel: { probes, p99Ms: sentinelP99Ms, failures: sentinelFailures.length },
		sidecarAfterAttacker,
		processTree: { before, afterAttacker, final },
		cgroupFinal: cgroupSnapshot(),
	};
	const path = writeArtifact("limits", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, sentinelP99Ms }));
	if (failures.length > 0) process.exitCode = 1;
}
