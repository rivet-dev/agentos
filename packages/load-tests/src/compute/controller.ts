// External Rivet Compute load generator.
//
// Runs OUTSIDE the target Compute deployment (in its own 1-CPU/1-GiB bounded
// container, via `just load-test-compute`) so target saturation cannot hide or
// coordinate the offered load. It drives the runner-scaling axis directly
// through the Rivet Engine management API (the RivetKit client's remote/auth
// wiring is deployment-specific; the raw API is the documented, reliable path):
//   - ramps keyed `agentosLoadRunner` actors through calibrated steps,
//   - measures actor create-to-ready latency,
//   - samples the runner census (active/stopped/slots),
//   - enforces hard ceilings for actor count, duration, and cleanup deadline,
//   - has a local kill switch and a bounded cleanup deadline,
//   - destroys every created actor and confirms runners drain,
//   - redacts every credential from logs and artifacts.
//
// Credentials come only from the environment (see docs-internal checklist K):
//   RIVET_ENDPOINT         https://<ns>:sk_...@api.rivet.dev   (secret; census)
//   RIVET_PUBLIC_ENDPOINT  https://<ns>:pk_...@api.rivet.dev   (public; actors)
//   RIVET_RUN_URL          https://<ns>.rivet.run              (gateway health)
// They are never printed, committed, or written to an artifact.
import {
	errorText,
	integerEnv,
	newRunId,
	numberEnv,
	percentile,
	runtimeProvenance,
	sleep,
	withTimeout,
	writeArtifact,
} from "../common.js";

/** Raised when a required Compute credential/endpoint is absent. */
class MissingComputeCredentialsError extends Error {
	constructor(names: string[]) {
		super(
			`missing required Compute credentials: ${names.join(", ")}; ` +
				`export them in the invoking environment (never commit them)`,
		);
		this.name = "MissingComputeCredentialsError";
	}
}

interface ParsedEndpoint {
	namespace: string;
	token: string;
	origin: string;
}

/** Parse `https://<namespace>:<token>@host` into parts; token stays in memory. */
function parseEndpoint(raw: string): ParsedEndpoint {
	const url = new URL(raw);
	const namespace = decodeURIComponent(url.username);
	const token = decodeURIComponent(url.password);
	if (!namespace || !token) {
		throw new Error("endpoint must be https://<namespace>:<token>@host");
	}
	url.username = "";
	url.password = "";
	return { namespace, token, origin: url.origin };
}

/** Redact any token-looking substring or userinfo from text. */
function redact(text: string): string {
	return text
		.replace(/\b(sk|pk|cloud)_[A-Za-z0-9._-]+/g, "$1_***REDACTED***")
		.replace(/\/\/[^/@\s]+:[^/@\s]+@/g, "//***:***@");
}

interface RunnerCensus {
	timestampMs: number;
	activeRunners: number;
	stoppedRunners: number;
	totalRunners: number;
	remainingSlots: number | null;
	totalSlots: number | null;
	error?: string;
}

async function sampleRunners(
	api: string,
	namespace: string,
	secretToken: string,
	timeoutMs: number,
): Promise<RunnerCensus> {
	const now = Date.now();
	try {
		const url =
			`${api}/runners?namespace=${encodeURIComponent(namespace)}` +
			`&name=default&include_stopped=true&limit=100`;
		const res = await withTimeout(
			"runner census",
			fetch(url, { headers: { Authorization: `Bearer ${secretToken}` } }),
			timeoutMs,
		);
		if (!res.ok) {
			return {
				timestampMs: now,
				activeRunners: 0,
				stoppedRunners: 0,
				totalRunners: 0,
				remainingSlots: null,
				totalSlots: null,
				error: `runner census HTTP ${res.status}`,
			};
		}
		const body = (await res.json()) as {
			runners?: Array<{
				stopped_at?: number | null;
				remaining_slots?: number | null;
				total_slots?: number | null;
			}>;
		};
		const runners = body.runners ?? [];
		const active = runners.filter((r) => r.stopped_at == null);
		return {
			timestampMs: now,
			activeRunners: active.length,
			stoppedRunners: runners.length - active.length,
			totalRunners: runners.length,
			remainingSlots: active.reduce((s, r) => s + (r.remaining_slots ?? 0), 0),
			totalSlots: active.reduce((s, r) => s + (r.total_slots ?? 0), 0),
		};
	} catch (error) {
		return {
			timestampMs: now,
			activeRunners: 0,
			stoppedRunners: 0,
			totalRunners: 0,
			remainingSlots: null,
			totalSlots: null,
			error: redact(errorText(error)),
		};
	}
}

/** Create a keyed actor via the Engine API; returns its actor id. */
async function createActor(
	api: string,
	namespace: string,
	publicToken: string,
	actorName: string,
	key: string,
	timeoutMs: number,
): Promise<string> {
	const res = await withTimeout(
		"actor create",
		fetch(`${api}/actors?namespace=${encodeURIComponent(namespace)}`, {
			method: "POST",
			headers: {
				Authorization: `Bearer ${publicToken}`,
				"Content-Type": "application/json",
			},
			body: JSON.stringify({
				name: actorName,
				key,
				runner_name_selector: "default",
				crash_policy: "restart",
			}),
		}),
		timeoutMs,
	);
	if (!res.ok) {
		throw new Error(`actor create HTTP ${res.status}: ${redact((await res.text()).slice(0, 200))}`);
	}
	const body = (await res.json()) as {
		actor?: { actor_id?: string; id?: string };
		actor_id?: string;
		id?: string;
	};
	const id = body.actor?.actor_id ?? body.actor?.id ?? body.actor_id ?? body.id;
	if (!id) throw new Error(`actor create returned no id: ${redact(JSON.stringify(body).slice(0, 200))}`);
	return id;
}

/** Poll an actor's gateway health until 200 or the deadline. */
async function waitActorReady(
	api: string,
	publicToken: string,
	actorId: string,
	deadlineMs: number,
): Promise<boolean> {
	while (Date.now() < deadlineMs) {
		try {
			const res = await withTimeout(
				"actor health",
				fetch(`${api}/gateway/${actorId}/health`, {
					headers: { "x-rivet-token": publicToken },
				}),
				8_000,
			);
			if (res.ok) return true;
		} catch {
			// transient; keep polling
		}
		await sleep(1_000);
	}
	return false;
}

/** Call a bounded actor action over the gateway (e.g. execArgv). Returns the
 * KernelExecResult in `.output`, or an error. Wire shape reverse-engineered from
 * rivetkit: POST /gateway/<id>/action/<name>, x-rivet-token + x-rivet-encoding:json,
 * body {args:[command, argv[]]}. */
async function callAction(
	api: string,
	publicToken: string,
	actorId: string,
	command: string,
	argv: string[],
	timeoutMs: number,
): Promise<{ ok: boolean; exitCode?: number; stdout?: string; stderr?: string; error?: string }> {
	try {
		const res = await withTimeout(
			"actor action",
			fetch(`${api}/gateway/${actorId}/action/execArgv`, {
				method: "POST",
				headers: {
					"x-rivet-token": publicToken,
					"x-rivet-encoding": "json",
					"Content-Type": "application/json",
				},
				body: JSON.stringify({ args: [command, argv, { timeout: Math.min(timeoutMs, 18_000) }] }),
			}),
			timeoutMs,
		);
		if (!res.ok) return { ok: false, error: `action HTTP ${res.status}: ${redact((await res.text()).slice(0, 160))}` };
		const body = (await res.json()) as {
			output?: { exitCode?: number; stdout?: string; stderr?: string };
			exitCode?: number;
			stdout?: string;
			stderr?: string;
		};
		const out = body.output ?? body;
		return { ok: true, exitCode: out.exitCode, stdout: out.stdout, stderr: out.stderr };
	} catch (error) {
		return { ok: false, error: redact(errorText(error)) };
	}
}

interface ActorCensus {
	timestampMs: number;
	total: number;
	pendingAllocation: number;
	connectable: number;
	sleeping: number;
	live: number;
	error?: string;
}

/** Census the actor fleet via /actors list + lifecycle timestamps (the real
 * scaling signal, since /runners is empty — LT-014). */
async function sampleActors(
	api: string,
	namespace: string,
	token: string,
	actorName: string,
	timeoutMs: number,
): Promise<ActorCensus> {
	const now = Date.now();
	try {
		const url =
			`${api}/actors?namespace=${encodeURIComponent(namespace)}` +
			`&name=${encodeURIComponent(actorName)}&include_destroyed=false&limit=100`;
		const res = await withTimeout(
			"actor census",
			fetch(url, { headers: { Authorization: `Bearer ${token}` } }),
			timeoutMs,
		);
		if (!res.ok) {
			return { timestampMs: now, total: 0, pendingAllocation: 0, connectable: 0, sleeping: 0, live: 0, error: `HTTP ${res.status}` };
		}
		const body = (await res.json()) as {
			actors?: Array<{
				pending_allocation_ts?: number | null;
				connectable_ts?: number | null;
				sleep_ts?: number | null;
				destroy_ts?: number | null;
			}>;
		};
		const actors = (body.actors ?? []).filter((a) => a.destroy_ts == null);
		return {
			timestampMs: now,
			total: actors.length,
			pendingAllocation: actors.filter((a) => a.pending_allocation_ts != null && a.connectable_ts == null).length,
			connectable: actors.filter((a) => a.connectable_ts != null && a.sleep_ts == null).length,
			sleeping: actors.filter((a) => a.sleep_ts != null).length,
			live: actors.length,
		};
	} catch (error) {
		return { timestampMs: now, total: 0, pendingAllocation: 0, connectable: 0, sleeping: 0, live: 0, error: redact(errorText(error)) };
	}
}

/** Destroy an actor via the Engine API. DELETE requires the SECRET token
 * (the publishable token gets 403 insufficient_permissions). */
async function destroyActor(
	api: string,
	namespace: string,
	secretToken: string,
	actorId: string,
): Promise<boolean> {
	try {
		const res = await withTimeout(
			"actor destroy",
			fetch(`${api}/actors/${actorId}?namespace=${encodeURIComponent(namespace)}`, {
				method: "DELETE",
				headers: { Authorization: `Bearer ${secretToken}` },
			}),
			10_000,
		);
		return res.ok;
	} catch {
		return false;
	}
}

interface StepResult {
	actors: number;
	createReadyMsP50: number;
	createReadyMsP99: number;
	readyCount: number;
	failedCount: number;
	censusAfterRamp: RunnerCensus;
	censusAfterHold: RunnerCensus;
	peakActiveRunners: number;
}

const TRIVIAL_SCRIPT = "process.stdout.write('ok')";
// Overflow the server's 64 MiB maxFilesystemBytes → trips the LT-011 crash class.
const FS_CRASH_SCRIPT =
	"const fs=require('node:fs');const c=Buffer.alloc(1024*1024,65);const fd=fs.openSync('/tmp/fill.bin','w');for(let i=0;i<96;i++)fs.writeSync(fd,c);fs.closeSync(fd);process.stdout.write('wrote')";

/**
 * Noisy-neighbor blast-radius test: fill a couple of runners with actors,
 * activate each actor's VM, then make ONE actor run the LT-011 filesystem-crash
 * workload and immediately re-probe ALL actors. If co-located actors fail their
 * next action, one guest crashing the shared runner sidecar is a cross-tenant
 * DoS at Compute scale.
 */
async function runNoisyNeighbor(
	runId: string,
	api: string,
	namespace: string,
	pk: string,
	sk: string,
	actorName: string,
): Promise<void> {
	const n = integerEnv("COMPUTE_NN_ACTORS", 30);
	const readyTimeoutMs = integerEnv("COMPUTE_READY_TIMEOUT_MS", 45_000);
	const ids: string[] = [];
	const failures: string[] = [];
	let attackerId: string | undefined;
	let survivorsBefore = 0;
	let survivorsAfter = 0;
	let attackerResult = "";

	try {
		// Create + ready N actors.
		for (let i = 0; i < n; i += 1) {
			const id = await createActor(api, namespace, pk, actorName, `${runId}-nn${i}`, 20_000);
			ids.push(id);
		}
		await Promise.all(ids.map((id) => waitActorReady(api, pk, id, Date.now() + readyTimeoutMs)));

		// Activate each actor's VM + confirm it answers. SEQUENTIALLY — a runner's
		// executor pool = --cpu count (LT-020), so concurrent actions on a small
		// runner get rejected; serial probing gives each actor the executor.
		const before: boolean[] = [];
		for (const id of ids) {
			const r = await callAction(api, pk, id, "node", ["-e", TRIVIAL_SCRIPT], 25_000);
			before.push(r.ok && r.exitCode === 0);
		}
		survivorsBefore = before.filter(Boolean).length;

		// One actor trips the crash.
		attackerId = ids[0];
		const atk = await callAction(api, pk, attackerId!, "node", ["-e", FS_CRASH_SCRIPT], 30_000);
		attackerResult = atk.ok ? `exit=${atk.exitCode} out=${(atk.stdout ?? "").slice(0, 20)} err=${redact((atk.stderr ?? "").slice(0, 160))}` : `error=${atk.error}`;

		// Re-probe ALL actors (serially); co-located failures = blast radius.
		await sleep(2_000);
		const after: boolean[] = [];
		for (const id of ids) {
			const r = await callAction(api, pk, id, "node", ["-e", TRIVIAL_SCRIPT], 25_000);
			after.push(r.ok && r.exitCode === 0);
		}
		survivorsAfter = after.filter(Boolean).length;

		// Collateral = neighbors (excluding the attacker) that answered before but not after.
		let collateral = 0;
		for (let i = 1; i < ids.length; i += 1) if (before[i] && !after[i]) collateral += 1;
		if (collateral > 0) {
			failures.push(`blast radius: ${collateral} co-located actor(s) failed after one actor tripped the fs crash (LT-011 at scale)`);
		}
	} catch (error) {
		failures.push(`noisy-neighbor error: ${redact(errorText(error))}`);
	} finally {
		for (const id of ids) await destroyActor(api, namespace, sk, id);
	}

	const artifact = {
		runId,
		lane: "compute-noisy-neighbor",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		config: { actors: n, actorName },
		attacker: { id: attackerId, result: attackerResult },
		survivorsBefore,
		survivorsAfter,
		provenance: runtimeProvenance(),
	};
	const path = writeArtifact("compute", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, survivorsBefore, survivorsAfter, attacker: attackerResult.slice(0, 120) }));
	if (failures.length > 0) process.exitCode = 1;
}

/**
 * Sleep/wake persistence E2E: create actors, write a marker file in each VM,
 * let them go to sleep (poll sleep_ts), wake them, and verify the file survived.
 * This tests that an actor's AgentOS filesystem persists across the VM being
 * recreated on wake (server.ts recreates the VM lazily via ensureVm).
 */
async function runSleepWake(
	runId: string,
	api: string,
	namespace: string,
	pk: string,
	sk: string,
	actorName: string,
): Promise<void> {
	const n = integerEnv("COMPUTE_SW_ACTORS", 5);
	const sleepWaitMs = integerEnv("COMPUTE_SW_SLEEP_WAIT_MS", 240_000);
	const dir = process.env.COMPUTE_SW_DIR ?? "/workspace";
	const readyTimeoutMs = integerEnv("COMPUTE_READY_TIMEOUT_MS", 60_000);
	const ids: string[] = [];
	const failures: string[] = [];
	const markers = new Map<string, string>();
	let wroteOk = 0;
	let verifiedBeforeSleep = 0;
	let sleptCount = 0;
	let survivedAfterWake = 0;
	let sampleWakeRead = "";

	// The marker file path lives inside the VM.
	const writeScript = (m: string) =>
		`const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(`${dir}/sw-marker.txt`)}, ${JSON.stringify(m)});process.stdout.write('wrote')`;
	const readScript = `const fs=require('node:fs');try{process.stdout.write(fs.readFileSync(${JSON.stringify(`${dir}/sw-marker.txt`)},'utf8'))}catch(e){process.stdout.write('MISSING:'+(e.code||e.message))}`;

	try {
		for (let i = 0; i < n; i += 1) {
			const id = await createActor(api, namespace, pk, actorName, `${runId}-sw${i}`, 20_000);
			ids.push(id);
			markers.set(id, `marker-${runId}-${i}`);
		}
		for (const id of ids) await waitActorReady(api, pk, id, Date.now() + readyTimeoutMs);

		// Write a marker file in each (serial — executor pool may be small).
		for (const id of ids) {
			const w = await callAction(api, pk, id, "node", ["-e", writeScript(markers.get(id)!)], 25_000);
			if (w.ok && w.exitCode === 0 && w.stdout === "wrote") wroteOk += 1;
			const r = await callAction(api, pk, id, "node", ["-e", readScript], 25_000);
			if (r.ok && r.stdout === markers.get(id)) verifiedBeforeSleep += 1;
		}

		// Wait for actors to sleep (stop touching them; poll sleep_ts).
		const deadline = Date.now() + sleepWaitMs;
		while (Date.now() < deadline) {
			await sleep(10_000);
			const census = await sampleActors(api, namespace, sk, actorName, 10_000);
			sleptCount = census.sleeping;
			if (census.sleeping >= ids.length) break;
		}

		// Wake each actor with a read; verify the file survived.
		for (const id of ids) {
			const r = await callAction(api, pk, id, "node", ["-e", readScript], 30_000);
			if (r.ok && r.stdout === markers.get(id)) survivedAfterWake += 1;
			else if (!sampleWakeRead) sampleWakeRead = r.ok ? `stdout=${(r.stdout ?? "").slice(0, 40)}` : `error=${r.error}`;
		}

		if (wroteOk < ids.length) failures.push(`only ${wroteOk}/${ids.length} wrote the marker`);
		if (verifiedBeforeSleep < ids.length) failures.push(`only ${verifiedBeforeSleep}/${ids.length} read back the marker before sleep`);
		if (sleptCount === 0) failures.push(`no actors observed sleeping within ${sleepWaitMs}ms (sleep may be slow/disabled)`);
		if (survivedAfterWake < ids.length) {
			failures.push(`PERSISTENCE: only ${survivedAfterWake}/${ids.length} markers survived sleep→wake (sample: ${sampleWakeRead})`);
		}
	} catch (error) {
		failures.push(`sleep-wake error: ${redact(errorText(error))}`);
	} finally {
		for (const id of ids) await destroyActor(api, namespace, sk, id);
	}

	const artifact = {
		runId,
		lane: "compute-sleep-wake",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		config: { actors: n, dir, sleepWaitMs, actorName },
		metrics: { wroteOk, verifiedBeforeSleep, sleptCount, survivedAfterWake, total: ids.length },
		provenance: runtimeProvenance(),
	};
	const path = writeArtifact("compute", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, metrics: artifact.metrics }));
	if (failures.length > 0) process.exitCode = 1;
}

/**
 * Rivet actor-churn soak: for a bounded duration, repeatedly create→ready→
 * destroy small batches of actors. Verifies no actor/runner leak and a healthy
 * deployment under sustained lifecycle churn.
 */
async function runChurnSoak(
	runId: string,
	api: string,
	namespace: string,
	pk: string,
	sk: string,
	actorName: string,
	runUrl: string | undefined,
): Promise<void> {
	const durationMs = integerEnv("COMPUTE_SOAK_MS", 300_000);
	const batch = integerEnv("COMPUTE_SOAK_BATCH", 5);
	const readyTimeoutMs = integerEnv("COMPUTE_READY_TIMEOUT_MS", 45_000);
	const failures: string[] = [];
	let created = 0;
	let ready = 0;
	let destroyed = 0;
	let cycles = 0;
	let healthChecks = 0;
	let healthFailures = 0;
	const deadline = Date.now() + durationMs;

	while (Date.now() < deadline) {
		cycles += 1;
		const ids: string[] = [];
		for (let i = 0; i < batch; i += 1) {
			try {
				const id = await createActor(api, namespace, pk, actorName, `${runId}-c${cycles}-${i}`, 20_000);
				ids.push(id);
				created += 1;
			} catch {
				// create failure counted implicitly (fewer ids)
			}
		}
		for (const id of ids) {
			if (await waitActorReady(api, pk, id, Date.now() + readyTimeoutMs)) ready += 1;
		}
		for (const id of ids) {
			if (await destroyActor(api, namespace, sk, id)) destroyed += 1;
		}
		// Periodic health check.
		if (runUrl) {
			try {
				const res = await withTimeout("health", fetch(`${runUrl}/api/rivet/health`), 8_000);
				healthChecks += 1;
				if (!res.ok) healthFailures += 1;
			} catch {
				healthChecks += 1;
				healthFailures += 1;
			}
		}
	}

	// Leak check: any actors from this run still alive?
	await sleep(3_000);
	const census = await sampleActors(api, namespace, sk, actorName, 10_000);
	const leaked = census.live;

	if (created > 0 && destroyed < created) failures.push(`destroyed ${destroyed} < created ${created}`);
	if (leaked > 0) failures.push(`${leaked} actor(s) still live after soak (possible leak)`);
	if (healthFailures > 0) failures.push(`${healthFailures}/${healthChecks} deployment health checks failed`);

	const artifact = {
		runId,
		lane: "compute-churn-soak",
		verdict: failures.length === 0 ? "pass" : "fail",
		failures,
		config: { durationMs, batch, actorName },
		metrics: { cycles, created, ready, destroyed, leaked, healthChecks, healthFailures },
		provenance: runtimeProvenance(),
	};
	const path = writeArtifact("compute", runId, artifact);
	console.log(JSON.stringify({ verdict: artifact.verdict, failures, path, metrics: artifact.metrics }));
	if (failures.length > 0) process.exitCode = 1;
}

export async function runComputeLoad(): Promise<void> {
	const runId = newRunId("compute");
	const actorName = process.env.COMPUTE_ACTOR_NAME ?? "agentosLoadRunner";
	// Hard safety ceiling on offered load. Raised for hundreds-of-actors scaling
	// tests; the controller still create-storms with bounded concurrency and
	// destroys everything within the cleanup deadline.
	const MAX_ACTORS = integerEnv("COMPUTE_MAX_ACTORS", 600);
	const steps = (process.env.COMPUTE_STEPS ?? "1,5,10")
		.split(",")
		.map((s) => Number.parseInt(s.trim(), 10));
	if (steps.some((n) => !Number.isSafeInteger(n) || n < 1 || n > MAX_ACTORS)) {
		throw new Error(`COMPUTE_STEPS must be comma-separated integers in 1..${MAX_ACTORS}`);
	}
	const holdMs = integerEnv("COMPUTE_HOLD_MS", 20_000);
	const scaleDownMs = integerEnv("COMPUTE_SCALE_DOWN_MS", 60_000);
	const createConcurrency = integerEnv("COMPUTE_CREATE_CONCURRENCY", 8);
	const readyTimeoutMs = integerEnv("COMPUTE_READY_TIMEOUT_MS", 45_000);
	const cleanupDeadlineMs = integerEnv("COMPUTE_CLEANUP_DEADLINE_MS", 90_000);
	const censusTimeoutMs = numberEnv("COMPUTE_CENSUS_TIMEOUT_MS", 10_000);

	// Credentials: fail fast and record a blocker rather than proceeding blind.
	const runUrl = process.env.RIVET_RUN_URL;
	const publicEndpointRaw = process.env.RIVET_PUBLIC_ENDPOINT;
	const secretEndpointRaw = process.env.RIVET_ENDPOINT;
	const missing: string[] = [];
	if (!publicEndpointRaw) missing.push("RIVET_PUBLIC_ENDPOINT");
	if (!secretEndpointRaw) missing.push("RIVET_ENDPOINT");
	if (missing.length > 0) {
		const artifact = {
			runId,
			lane: "compute-scaling",
			verdict: "blocked",
			reason: "missing Compute credentials/endpoints",
			missing,
			config: { steps, holdMs, scaleDownMs },
			provenance: runtimeProvenance(),
		};
		const path = writeArtifact("compute", runId, artifact);
		console.log(JSON.stringify({ verdict: "blocked", missing, path }));
		throw new MissingComputeCredentialsError(missing);
	}

	const secret = parseEndpoint(secretEndpointRaw!);
	const pub = parseEndpoint(publicEndpointRaw!);
	const api = pub.origin; // https://api.rivet.dev
	const namespace = pub.namespace;

	// Attack #1: does one actor tripping the LT-011 sidecar crash take down its
	// co-located actors (shared-runner blast radius)?
	const mode = process.env.COMPUTE_MODE ?? "scale";
	if (mode === "noisy-neighbor") {
		await runNoisyNeighbor(runId, api, namespace, pub.token, secret.token, actorName);
		return;
	}
	// Sleep/wake persistence: spawn actors, write a file, let them sleep, wake
	// them, verify the file survived.
	if (mode === "sleep-wake") {
		await runSleepWake(runId, api, namespace, pub.token, secret.token, actorName);
		return;
	}
	// Rivet soak: rapid create→ready→destroy churn for a duration; verify no
	// leaked actors and the deployment stays healthy.
	if (mode === "churn") {
		await runChurnSoak(runId, api, namespace, pub.token, secret.token, actorName, runUrl);
		return;
	}

	let aborted = false;
	const abort = () => {
		aborted = true;
	};
	process.once("SIGINT", abort);
	process.once("SIGTERM", abort);

	const created = new Map<number, string>(); // index -> actorId
	const stepResults: StepResult[] = [];
	const censusSeries: RunnerCensus[] = [];
	const actorCensusSeries: ActorCensus[] = [];
	let topLevelError: string | undefined;
	let peakActiveRunners = 0;
	let peakConnectable = 0;
	let peakPending = 0;

	const census = async () => {
		// Runner census (empty for this deployment — LT-014) plus the actor
		// lifecycle census, which IS the real scaling signal.
		const [c, a] = await Promise.all([
			sampleRunners(api, namespace, secret.token, censusTimeoutMs),
			sampleActors(api, namespace, secret.token, actorName, censusTimeoutMs),
		]);
		peakActiveRunners = Math.max(peakActiveRunners, c.activeRunners);
		peakConnectable = Math.max(peakConnectable, a.connectable);
		peakPending = Math.max(peakPending, a.pendingAllocation);
		censusSeries.push(c);
		actorCensusSeries.push(a);
		return c;
	};

	async function pool<T>(items: number[], limit: number, fn: (i: number) => Promise<T>): Promise<T[]> {
		const out: T[] = [];
		let cursor = 0;
		const worker = async () => {
			while (cursor < items.length && !aborted) {
				const i = items[cursor++]!;
				out.push(await fn(i));
			}
		};
		await Promise.all(Array.from({ length: Math.min(limit, items.length) }, () => worker()));
		return out;
	}

	try {
		// Baseline census before any load.
		const baselineCensus = await census();

		for (const actors of steps) {
			if (aborted) break;
			const indices = Array.from({ length: actors }, (_, i) => i);
			const readyMs: number[] = [];
			let failed = 0;

			await pool(indices, createConcurrency, async (index) => {
				const key = `${runId}-a${index}`;
				const startedAt = Date.now();
				try {
					if (!created.has(index)) {
						const id = await createActor(api, namespace, pub.token, actorName, key, 20_000);
						created.set(index, id);
					}
					const ready = await waitActorReady(api, pub.token, created.get(index)!, Date.now() + readyTimeoutMs);
					if (ready) readyMs.push(Date.now() - startedAt);
					else failed += 1;
				} catch (error) {
					failed += 1;
					if (failed <= 2) topLevelError = redact(errorText(error));
				}
			});

			const censusAfterRamp = await census();

			const holdEnd = Date.now() + holdMs;
			while (Date.now() < holdEnd && !aborted) {
				await census();
				await sleep(2_000);
			}
			const censusAfterHold = await census();

			stepResults.push({
				actors,
				createReadyMsP50: percentile(readyMs, 0.5),
				createReadyMsP99: percentile(readyMs, 0.99),
				readyCount: readyMs.length,
				failedCount: failed,
				censusAfterRamp,
				censusAfterHold,
				peakActiveRunners,
			});
			void baselineCensus;
		}
	} catch (error) {
		topLevelError = redact(errorText(error));
	} finally {
		// Cleanup: destroy every created actor within the bounded deadline.
		const cleanupStart = Date.now();
		for (const [, actorId] of created) {
			if (Date.now() - cleanupStart > cleanupDeadlineMs) break;
			await destroyActor(api, namespace, secret.token, actorId);
		}
	}

	// Drain: sample until runners return to their idle floor or the deadline.
	const drainStart = Date.now();
	let finalCensus = await census();
	let drainMs: number | null = null;
	while (finalCensus.activeRunners > 0 && Date.now() - drainStart < scaleDownMs && !aborted) {
		await sleep(3_000);
		finalCensus = await census();
	}
	if (finalCensus.activeRunners === 0) drainMs = Date.now() - drainStart;

	const failures: string[] = [];
	if (topLevelError) failures.push(`controller error: ${topLevelError}`);
	if (stepResults.some((s) => s.failedCount > 0)) failures.push("one or more actors failed to become ready");
	if (finalCensus.activeRunners > 0) {
		failures.push(`${finalCensus.activeRunners} runner(s) still active after drain deadline`);
	}
	// NOTE (LT-014): the /runners census returns 0 for this serverless deployment,
	// so runner count is informational only — the scaling verdict rests on the
	// actor lifecycle (create-to-ready, health, destroy/drain), not runner count.
	const runnerCensusObserved = censusSeries.some((c) => c.totalRunners > 0);

	const artifact = {
		runId,
		lane: "compute-scaling",
		verdict: aborted ? "aborted" : failures.length === 0 ? "pass" : "fail",
		failures,
		config: {
			actorName,
			steps,
			holdMs,
			scaleDownMs,
			createConcurrency,
			readyTimeoutMs,
			cleanupDeadlineMs,
			maxActors: MAX_ACTORS,
		},
		endpoints: {
			namespace,
			api,
			runUrl: runUrl ? redact(runUrl) : undefined,
		},
		provenance: runtimeProvenance(),
		steps: stepResults,
		peakActiveRunners,
		peakConnectable,
		peakPending,
		runnerCensusObserved,
		drainMs,
		finalCensus,
		census: censusSeries,
		actorCensus: actorCensusSeries,
		createdActorCount: created.size,
	};
	const path = writeArtifact("compute", runId, artifact);
	console.log(
		JSON.stringify({
			verdict: artifact.verdict,
			failures,
			path,
			peakActiveRunners,
			peakConnectable,
			peakPending,
			drainMs,
			steps: stepResults.map((s) => ({ actors: s.actors, ready: s.readyCount, failed: s.failedCount, connectable: s.censusAfterRamp.activeRunners })),
		}),
	);
	if (failures.length > 0 && !aborted) process.exitCode = 1;
}
