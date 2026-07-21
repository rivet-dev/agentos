// Deterministic adversarial limit matrix (docs-internal/load-testing.md, lane A
// / checklist E). Generalises the single process-count probe into a table of
// resource-limit probes that share one sidecar + sentinel and one survival
// contract. Each probe overshoots its configured cap and asserts:
//   1. the configured boundary fires (typed limit error or Linux errno),
//   2. a near-limit host warning appears where that contract applies,
//   3. the shared sidecar stays ready and the sentinel keeps progressing,
//   4. the attacker VM is disposed and accounting returns to sentinel-only,
//   5. a fresh VM can still be created, and no container OOM occurred.
//
// Every probe overshoots in a single guest run: the first ~cap attempts succeed
// and the remainder are rejected, exercising below/at/above the boundary at once.
import {
	AgentOs,
	type AgentOsLimits,
	type AgentOsSidecar,
	type LimitWarning,
} from "@rivet-dev/agentos-core";
import {
	cgroupSnapshot,
	errorText,
	newRunId,
	percentile,
	runtimeProvenance,
	sampleProcessTree,
	sleep,
	type TimedProbe,
	withTimeout,
	writeArtifact,
} from "../common.js";

interface GuestOutcome {
	index?: number;
	status: "spawned" | "error" | "timeout";
	code?: string | null;
	message?: string;
	writtenBytes?: number;
}

interface LimitProbe {
	name: string;
	/** Human-readable configuration path named in the verdict. */
	limitPath: string;
	cap: number;
	attempts: number;
	limits: AgentOsLimits;
	/** CommonJS guest that overshoots and prints AGENTOS_LIMIT_RESULT=<json>. */
	guest: string;
	/** Matches the errno / typed-limit text a correct rejection must carry. */
	rejectionPattern: RegExp;
	/** Some limits emit a near-threshold warning; count-of-1 fill jumps do not. */
	expectWarning: boolean;
	/**
	 * "count": guest attempts N units and prints per-attempt outcomes; some are
	 * rejected. "kill": the single offending execution must be TERMINATED by the
	 * limit (typed error / non-zero exit), not run to a generic timeout.
	 * "crash-observe": run the workload and only assert the sidecar SURVIVES
	 * (fresh VM works, no SidecarProcessExited) — used to map crash-class vectors.
	 */
	kind?: "count" | "kill" | "crash-observe";
}

function processStormGuest(attempts: number): string {
	return `
const { spawn } = require("node:child_process");
const attempts = ${attempts};
const children = [];
function start(index) {
  return new Promise((resolve) => {
    let settled = false; let child;
    const finish = (v) => { if (settled) return; settled = true; resolve(v); };
    try { child = spawn(process.execPath, ["-e", "setTimeout(()=>{},30000)"], { stdio: "ignore" }); }
    catch (e) { finish({ index, status: "error", code: e.code ?? null, message: e.message }); return; }
    children.push(child);
    child.once("spawn", () => finish({ index, status: "spawned", pid: child.pid }));
    child.once("error", (e) => finish({ index, status: "error", code: e.code ?? null, message: e.message }));
    setTimeout(() => finish({ index, status: "timeout" }), 3000);
  });
}
(async () => {
  const outcomes = await Promise.all(Array.from({ length: attempts }, (_, i) => start(i)));
  for (const c of children) { if (c.pid) { try { c.kill("SIGKILL"); } catch {} } }
  await new Promise((r) => setTimeout(r, 100));
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify(outcomes) + "\\n");
})();
`;
}

function openFdGuest(attempts: number): string {
	return `
const fs = require("node:fs");
const attempts = ${attempts};
const fds = []; const outcomes = [];
for (let i = 0; i < attempts; i++) {
  try { const fd = fs.openSync("/tmp/fdprobe_" + i, "w"); fds.push(fd); outcomes.push({ index: i, status: "spawned" }); }
  catch (e) { outcomes.push({ index: i, status: "error", code: e.code ?? null, message: e.message }); }
}
for (const fd of fds) { try { fs.closeSync(fd); } catch {} }
process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify(outcomes) + "\\n");
`;
}

function socketGuest(attempts: number): string {
	return `
const net = require("node:net");
const attempts = ${attempts};
const servers = []; const outcomes = [];
function tryListen(i) {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (v) => { if (settled) return; settled = true; resolve(v); };
    let s;
    try { s = net.createServer(); } catch (e) { finish({ index: i, status: "error", code: e.code ?? null, message: e.message }); return; }
    // Permanent no-op error handler so a late async 'error' never crashes the
    // process (the resource-limit rejection can surface after we settle).
    s.on("error", (e) => { if (!settled) finish({ index: i, status: "error", code: e.code ?? null, message: e.message }); });
    try { s.listen(0, "127.0.0.1", () => { servers.push(s); finish({ index: i, status: "spawned" }); }); }
    catch (e) { finish({ index: i, status: "error", code: e.code ?? null, message: e.message }); }
    setTimeout(() => finish({ index: i, status: "timeout" }), 2000);
  });
}
process.on("uncaughtException", () => {}); // resource-limit errors may surface async post-settle
(async () => {
  for (let i = 0; i < attempts; i++) { outcomes.push(await tryListen(i)); }
  for (const s of servers) { try { s.close(); } catch {} }
  await new Promise((r) => setTimeout(r, 50));
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify(outcomes) + "\\n");
})();
`;
}

function filesystemGuest(capBytes: number): string {
	// Attempt to write ~4x the byte cap in 1 MiB chunks to a single file.
	const chunks = Math.ceil((capBytes * 4) / (1024 * 1024));
	return `
const fs = require("node:fs");
const chunk = Buffer.alloc(1024 * 1024, 65);
let written = 0; let error = null;
try {
  const fd = fs.openSync("/tmp/fill.bin", "w");
  for (let i = 0; i < ${chunks}; i++) { fs.writeSync(fd, chunk); written += chunk.length; }
  fs.closeSync(fd);
} catch (e) { error = { code: e.code ?? null, message: e.message }; }
const outcomes = error
  ? [{ status: "error", code: error.code, message: error.message, writtenBytes: written }]
  : [{ status: "spawned", writtenBytes: written }];
process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify(outcomes) + "\\n");
`;
}

// LT-011 crash-class vectors: does the sidecar crash on paths/APIs OTHER than
// the confirmed chunked-write-to-/tmp? maxFilesystemBytes is set high so ENOSPC
// does not preempt the crash — a sidecar death is the finding.
function fsWorkspaceGuest(): string {
	return `
const fs = require("node:fs");
const chunk = Buffer.alloc(1024 * 1024, 65);
try { const fd = fs.openSync("/workspace/fill.bin", "w"); for (let i=0;i<48;i++) fs.writeSync(fd, chunk); fs.closeSync(fd);
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"spawned"}]) + "\\n"); }
catch (e) { process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"error",code:e.code??null,message:String(e.message).slice(0,120)}]) + "\\n"); }
`;
}
function fsWriteFileGuest(): string {
	return `
const fs = require("node:fs");
try { fs.writeFileSync("/tmp/big.bin", Buffer.alloc(64*1024*1024, 66));
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"spawned"}]) + "\\n"); }
catch (e) { process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"error",code:e.code??null,message:String(e.message).slice(0,120)}]) + "\\n"); }
`;
}
function fsPwriteOffsetGuest(): string {
	return `
const fs = require("node:fs");
const chunk = Buffer.alloc(1024 * 1024, 67);
try { const fd = fs.openSync("/tmp/sparse.bin", "w"); fs.writeSync(fd, chunk, 0, chunk.length, 64*1024*1024); fs.closeSync(fd);
  process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"spawned"}]) + "\\n"); }
catch (e) { process.stdout.write("AGENTOS_LIMIT_RESULT=" + JSON.stringify([{status:"error",code:e.code??null,message:String(e.message).slice(0,120)}]) + "\\n"); }
`;
}

// Kill-style guests: each drives one execution into a JS-runtime limit.
const jsCpuGuest = `let x = 0; while (true) { x += Math.sqrt(x + 1); if (x < 0) break; }`;
const jsHeapGuest = `const a = []; while (true) { a.push(Buffer.alloc(1024 * 1024, 1)); }`;
const jsOutputGuest = `const b = Buffer.alloc(1024 * 1024, 65); for (let i = 0; i < 64; i++) { process.stdout.write(b); }`;
const compoundCpuOutputGuest = `const b = Buffer.alloc(256 * 1024, 66); let x = 0; while (true) { for (let i = 0; i < 1000; i++) x += Math.sqrt(x + 1); process.stdout.write(b); }`;

function buildProbes(): LimitProbe[] {
	const fsCap = 8 * 1024 * 1024;
	return [
		{
			name: "processes",
			limitPath: "limits.resources.maxProcesses",
			cap: 8,
			attempts: 32,
			limits: { resources: { maxProcesses: 8 } },
			guest: processStormGuest(32),
			rejectionPattern:
				/EAGAIN|resource temporarily unavailable|maxProcesses|process(?:es)?[^\n]*limit|ERR_AGENTOS/i,
			expectWarning: true,
		},
		{
			name: "open-fds",
			limitPath: "limits.resources.maxOpenFds",
			cap: 64,
			attempts: 160,
			limits: { resources: { maxOpenFds: 64 } },
			guest: openFdGuest(160),
			rejectionPattern: /EMFILE|ENFILE|too many open files|maxOpenFds|open[_ ]?fds?|ERR_AGENTOS/i,
			// fd near-limit warnings reach sidecar stderr only, not onLimitWarning (LT-010).
			expectWarning: false,
		},
		{
			name: "sockets",
			limitPath: "limits.resources.maxSockets",
			cap: 32,
			attempts: 96,
			limits: { resources: { maxSockets: 32 } },
			guest: socketGuest(96),
			rejectionPattern:
				/EMFILE|ENFILE|ENOBUFS|EADDRNOTAVAIL|too many|maxSockets|socket[^\n]*limit|ERR_AGENTOS_RESOURCE_LIMIT|ERR_AGENTOS/i,
			// socket near-limit warnings reach sidecar stderr only, not onLimitWarning (LT-010).
			expectWarning: false,
		},
		{
			name: "js-cpu",
			limitPath: "limits.jsRuntime.cpuTimeLimitMs",
			cap: 2_000,
			attempts: 1,
			limits: { jsRuntime: { cpuTimeLimitMs: 2_000, wallClockLimitMs: 0 } },
			guest: jsCpuGuest,
			rejectionPattern: /cpu|CPU|time limit|watchdog|terminat|ERR_AGENTOS/i,
			expectWarning: false,
			kind: "kill",
		},
		{
			name: "js-wallclock",
			limitPath: "limits.jsRuntime.wallClockLimitMs",
			cap: 2_000,
			attempts: 1,
			limits: { jsRuntime: { wallClockLimitMs: 2_000 } },
			guest: jsCpuGuest,
			rejectionPattern: /wall|clock|time limit|watchdog|terminat|deadline|ERR_AGENTOS/i,
			expectWarning: false,
			kind: "kill",
		},
		{
			name: "js-heap",
			limitPath: "limits.jsRuntime.v8HeapLimitMb",
			cap: 64,
			attempts: 1,
			limits: { jsRuntime: { v8HeapLimitMb: 64 } },
			guest: jsHeapGuest,
			rejectionPattern: /heap|memory|out of memory|oom|allocation|ERR_AGENTOS/i,
			expectWarning: false,
			kind: "kill",
		},
		{
			name: "js-output",
			limitPath: "limits.jsRuntime.capturedOutputLimitBytes",
			cap: 1024 * 1024,
			attempts: 1,
			limits: { jsRuntime: { capturedOutputLimitBytes: 1024 * 1024 } },
			guest: jsOutputGuest,
			rejectionPattern: /output|captured|stdout|byte|limit|ERR_AGENTOS/i,
			expectWarning: false,
			kind: "kill",
		},
		{
			// Compound: burn CPU and flood stdout at once; whichever cap is lower
			// must fire, and the offending execution must be terminated.
			name: "compound-cpu-output",
			limitPath: "limits.jsRuntime.cpuTimeLimitMs+capturedOutputLimitBytes",
			cap: 3_000,
			attempts: 1,
			limits: {
				jsRuntime: {
					cpuTimeLimitMs: 3_000,
					wallClockLimitMs: 0,
					capturedOutputLimitBytes: 2 * 1024 * 1024,
				},
			},
			guest: compoundCpuOutputGuest,
			rejectionPattern:
				/cpu|CPU|time limit|output|captured|stdout|byte|limit|watchdog|terminat|ERR_AGENTOS/i,
			expectWarning: false,
			kind: "kill",
		},
		// LT-011 crash-class mapping (run each ISOLATED via LOAD_TEST_MATRIX_ONLY;
		// high maxFilesystemBytes so a crash — not ENOSPC — is the signal).
		{
			name: "fs-crash-workspace",
			limitPath: "LT-011 vector: /workspace chunked write",
			cap: 256 * 1024 * 1024,
			attempts: 1,
			limits: { resources: { maxFilesystemBytes: 256 * 1024 * 1024 } },
			guest: fsWorkspaceGuest(),
			rejectionPattern: /.*/,
			expectWarning: false,
			kind: "crash-observe",
		},
		{
			name: "fs-crash-writefile64",
			limitPath: "LT-011 vector: single 64MiB writeFileSync to /tmp",
			cap: 256 * 1024 * 1024,
			attempts: 1,
			limits: { resources: { maxFilesystemBytes: 256 * 1024 * 1024 } },
			guest: fsWriteFileGuest(),
			rejectionPattern: /.*/,
			expectWarning: false,
			kind: "crash-observe",
		},
		{
			name: "fs-crash-pwrite-offset",
			limitPath: "LT-011 vector: pwrite at 64MiB offset (/tmp sparse)",
			cap: 256 * 1024 * 1024,
			attempts: 1,
			limits: { resources: { maxFilesystemBytes: 256 * 1024 * 1024 } },
			guest: fsPwriteOffsetGuest(),
			rejectionPattern: /.*/,
			expectWarning: false,
			kind: "crash-observe",
		},
		{
			// Runs LAST: this probe crashes the shared sidecar (LT-011), so it must
			// not precede probes that need a healthy sidecar.
			name: "filesystem-bytes",
			limitPath: "limits.resources.maxFilesystemBytes",
			cap: fsCap,
			attempts: 1,
			limits: { resources: { maxFilesystemBytes: fsCap } },
			guest: filesystemGuest(fsCap),
			rejectionPattern:
				/ENOSPC|EDQUOT|EFBIG|no space|disk quota|maxFilesystemBytes|filesystem[^\n]*limit|ERR_AGENTOS/i,
			expectWarning: false,
		},
	];
}

async function probeSentinel(vm: AgentOs): Promise<TimedProbe> {
	const startedAtMs = Date.now();
	try {
		const result = await withTimeout(
			"sentinel exec",
			vm.execArgv("node", ["-e", 'process.stdout.write("sentinel-ok")'], { timeout: 5_000 }),
			7_000,
		);
		if (result.exitCode !== 0 || result.stdout !== "sentinel-ok") {
			throw new Error(`sentinel exit=${result.exitCode} stderr=${JSON.stringify(result.stderr)}`);
		}
		return { startedAtMs, durationMs: Date.now() - startedAtMs, ok: true };
	} catch (error) {
		return { startedAtMs, durationMs: Date.now() - startedAtMs, ok: false, error: errorText(error) };
	}
}

function parseOutcomes(stdout: string): GuestOutcome[] {
	const line = stdout.split("\n").find((l) => l.startsWith("AGENTOS_LIMIT_RESULT="));
	if (!line) return [];
	try {
		const parsed = JSON.parse(line.slice("AGENTOS_LIMIT_RESULT=".length));
		return Array.isArray(parsed) ? parsed : [];
	} catch {
		return [];
	}
}

interface ProbeResult {
	name: string;
	limitPath: string;
	verdict: "pass" | "fail";
	failures: string[];
	cap: number;
	attempts: number;
	spawned: number;
	rejected: number;
	rejectionSample: string;
	warnings: number;
	sentinelFailuresDuring: number;
	freshVmOk: boolean;
}

async function runProbe(
	sidecar: AgentOsSidecar,
	sentinel: AgentOs,
	probe: LimitProbe,
): Promise<ProbeResult> {
	const failures: string[] = [];
	const warnings: LimitWarning[] = [];
	let attacker: AgentOs | undefined;
	let sentinelFailuresDuring = 0;
	let spawned = 0;
	let rejected = 0;
	let rejectionSample = "";
	let freshVmOk = false;

	try {
		attacker = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
			limits: probe.limits,
			onLimitWarning: (w) => warnings.push(w),
		});

		let attackExit: number | undefined;
		let attackStderr = "";
		let attackStdout = "";
		let attackThrew: string | undefined;
		try {
			const attack = await withTimeout(
				`${probe.name} storm`,
				attacker.execArgv("node", ["-e", probe.guest], { timeout: 25_000 }),
				30_000,
			);
			attackExit = attack.exitCode;
			attackStderr = attack.stderr;
			attackStdout = attack.stdout;
		} catch (error) {
			attackThrew = errorText(error);
		}
		// Sentinel must have kept working during pressure.
		const during = await probeSentinel(sentinel);
		if (!during.ok) sentinelFailuresDuring += 1;

		if (probe.kind === "crash-observe") {
			// Map crash-class vectors: the ONLY failure is the sidecar dying.
			const evidence = `${attackThrew ?? ""} exit=${attackExit ?? "?"} ${attackStderr}`.slice(0, 240);
			rejectionSample = `${parseOutcomes(attackStdout).map((o) => o.status).join(",")} | ${evidence}`;
			if (attackThrew && /SidecarProcessExited|Kernel\(|EBADF|sidecar process exited/i.test(attackThrew)) {
				failures.push(`SIDECAR CRASHED on ${probe.name}: ${attackThrew.slice(0, 160)}`);
			}
			// freshVmOk / sentinel checks below catch a crash even if this exec
			// returned before the sidecar fully died.
		} else if ((probe.kind ?? "count") === "kill") {
			// The offending execution must be terminated by the typed limit — not
			// left to run into the generic guest/exec timeout.
			const evidence = `${attackThrew ?? ""} exit=${attackExit ?? "?"} ${attackStderr}`;
			const terminated = attackThrew !== undefined || (attackExit !== undefined && attackExit !== 0);
			rejectionSample = evidence.slice(0, 240);
			if (!terminated) {
				failures.push(`${probe.limitPath} did not terminate the offending execution (exit=0)`);
			} else if (/timed out after \d+ms/.test(evidence) || !probe.rejectionPattern.test(evidence)) {
				failures.push(`termination did not name ${probe.limitPath} (generic/untyped): ${rejectionSample}`);
			} else {
				rejected = 1;
			}
		} else {
			const outcomes = parseOutcomes(attackStdout);
			const rejections = outcomes.filter((o) => o.status === "error");
			spawned = outcomes.filter((o) => o.status === "spawned").length;
			rejected = rejections.length;
			rejectionSample = JSON.stringify(rejections.slice(0, 3));
			if (attackExit !== 0 && attackExit !== undefined) {
				failures.push(`attack parent exited ${attackExit}: ${attackStderr.slice(0, 200)}`);
			}
			if (attackThrew) failures.push(`attack threw: ${attackThrew.slice(0, 200)}`);
			if (rejected === 0) {
				failures.push(`no attempt was rejected (${probe.limitPath} did not fire)`);
			} else if (!probe.rejectionPattern.test(rejectionSample)) {
				failures.push(`rejection did not name ${probe.limitPath}: ${rejectionSample.slice(0, 200)}`);
			}
			if (probe.expectWarning && warnings.length === 0) {
				failures.push(`no near-limit warning for ${probe.limitPath}`);
			}
		}
	} catch (error) {
		failures.push(`probe threw: ${errorText(error)}`);
	} finally {
		if (attacker) await attacker.dispose().catch(() => {});
	}

	// A fresh VM must still be creatable + runnable after the attack.
	try {
		const fresh = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
		});
		const r = await withTimeout(
			"fresh vm",
			fresh.execArgv("node", ["-e", 'process.stdout.write("fresh-ok")'], { timeout: 5_000 }),
			7_000,
		);
		freshVmOk = r.exitCode === 0 && r.stdout === "fresh-ok";
		await fresh.dispose().catch(() => {});
	} catch (error) {
		failures.push(`fresh VM after ${probe.name} failed: ${errorText(error)}`);
	}
	if (!freshVmOk) failures.push(`fresh VM after ${probe.name} did not run`);

	return {
		name: probe.name,
		limitPath: probe.limitPath,
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		cap: probe.cap,
		attempts: probe.attempts,
		spawned,
		rejected,
		rejectionSample,
		warnings: warnings.length,
		sentinelFailuresDuring,
		freshVmOk,
	};
}

export async function runLimitMatrix(): Promise<void> {
	const runId = newRunId("limit-matrix");
	// Optional isolation: LOAD_TEST_MATRIX_ONLY=filesystem-bytes runs one probe
	// against a fresh sidecar (used to isolate a cross-probe interaction).
	const only = process.env.LOAD_TEST_MATRIX_ONLY;
	const probes = only
		? buildProbes().filter((p) => only.split(",").includes(p.name))
		: buildProbes();
	if (probes.length === 0) throw new Error(`LOAD_TEST_MATRIX_ONLY matched no probe: ${only}`);
	let sidecar: AgentOsSidecar | undefined;
	let sentinel: AgentOs | undefined;
	const results: ProbeResult[] = [];
	const sentinelProbes: TimedProbe[] = [];
	const failures: string[] = [];
	const before = sampleProcessTree();

	try {
		sidecar = await AgentOs.createSidecar({ sidecarId: `load-matrix-${runId}` });
		sentinel = await AgentOs.create({
			defaultSoftware: false,
			sidecar: { kind: "explicit", handle: sidecar },
		});
		for (let i = 0; i < 3; i += 1) sentinelProbes.push(await probeSentinel(sentinel));

		for (const probe of probes) {
			results.push(await runProbe(sidecar, sentinel, probe));
			sentinelProbes.push(await probeSentinel(sentinel));
			const desc = sidecar.describe();
			if (desc.state !== "ready") failures.push(`sidecar state ${desc.state} after ${probe.name}`);
			if (desc.activeVmCount !== 1) {
				failures.push(`after ${probe.name}: activeVmCount ${desc.activeVmCount}, expected sentinel-only`);
			}
		}
	} catch (error) {
		failures.push(`top-level: ${errorText(error)}`);
	} finally {
		if (sentinel) await sentinel.dispose().catch(() => {});
		if (sidecar) await sidecar.dispose().catch(() => {});
	}

	await sleep(300);
	const final = sampleProcessTree();
	const sentinelFailures = sentinelProbes.filter((p) => !p.ok).length;
	if (sentinelFailures > 0) failures.push(`${sentinelFailures} sentinel probes failed`);
	const oomKills = Number(cgroupSnapshot()["memory.events.oom_kill"] ?? 0);
	if (oomKills > 0) failures.push(`container recorded ${oomKills} OOM kill(s)`);
	for (const r of results) if (r.verdict === "fail") failures.push(`probe ${r.name}: ${r.failures.join("; ")}`);

	const artifact = {
		runId,
		lane: "adversarial-limit-matrix",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		provenance: runtimeProvenance(),
		results,
		sentinel: {
			probes: sentinelProbes.length,
			failures: sentinelFailures,
			p99Ms: percentile(sentinelProbes.filter((p) => p.ok).map((p) => p.durationMs), 0.99),
		},
		processTree: { before, final },
		cgroupFinal: cgroupSnapshot(),
	};
	const path = writeArtifact("limit-matrix", runId, artifact);
	console.log(
		JSON.stringify({
			verdict: artifact.verdict,
			failures: failures.slice(0, 8),
			path,
			probes: results.map((r) => ({ name: r.name, verdict: r.verdict, spawned: r.spawned, rejected: r.rejected })),
		}),
	);
	if (failures.length > 0) process.exitCode = 1;
}
