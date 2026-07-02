// Standalone "dummy example" driver for the agentOS RivetKit integration.
//
// NOT a test — run it directly with node:
//   node packages/agentos/tests/fixtures/demo.mjs
//
// It boots the existing `agentos-runtime-server.ts` fixture (a RivetKit
// registry built with `setup({ use: { os: agentOs({...}) } })`, native
// runtime, backed by the sibling ../r6 rivet-engine), then connects a client,
// creates a VM actor, and does a dummy writeFile/readFile round-trip.
//
// This is a plain-JS port of the `AGENTOS_E2E_FULL=1` e2e case in actor.test.ts.

import { spawn } from "node:child_process";
import { appendFileSync, existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createClient } from "rivetkit/client";

const fixtureDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(fixtureDir, "../../../.."); // .../inspector-tabs
const r6Root = resolve(repoRoot, "../r6");
const r6RivetkitPackageRoot = join(
	r6Root,
	"rivetkit-typescript",
	"packages",
	"rivetkit",
);
const tsxLoaderPath = join(
	r6RivetkitPackageRoot,
	"node_modules",
	"tsx",
	"dist",
	"loader.mjs",
);
const runtimeFixturePath = join(fixtureDir, "agentos-runtime-server.ts");

const sidecarPath = join(repoRoot, "target", "debug", "agentos-sidecar");
const pluginPath = join(repoRoot, "target", "debug", "libagentos_actor_plugin.so");
const engineBinary = join(r6Root, "target", "debug", "rivet-engine");

for (const [label, p] of [
	["sidecar", sidecarPath],
	["actor-plugin cdylib", pluginPath],
	["rivet-engine", engineBinary],
	["r6 tsx loader", tsxLoaderPath],
]) {
	if (!existsSync(p)) {
		console.error(`[demo] missing ${label}: ${p}`);
		process.exit(1);
	}
}

const log = (...a) => console.log("[demo]", ...a);

function bytesToString(value) {
	if (value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
	if (Array.isArray(value)) return Buffer.from(value).toString("utf8");
	if (typeof value === "string") return value;
	throw new Error(`unexpected readFile result: ${String(value)}`);
}

let runtime;
let runtimeLogs = { stdout: "", stderr: "" };
const childOutput = () =>
	[runtimeLogs.stdout, runtimeLogs.stderr].filter(Boolean).join("\n");

async function waitForHealth(endpoint, timeoutMs) {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		if (runtime && runtime.exitCode !== null) {
			throw new Error(`runtime exited before health:\n${childOutput()}`);
		}
		try {
			const r = await fetch(`${endpoint}/health`);
			if (r.ok) return;
		} catch {}
		await new Promise((r) => setTimeout(r, 500));
	}
	throw new Error(`timed out waiting for engine health\n${childOutput()}`);
}

async function upsertNormalRunnerConfig(endpoint, namespace, token, poolName) {
	const authHeaders = token ? { Authorization: `Bearer ${token}` } : {};
	const dcRes = await fetch(
		`${endpoint}/datacenters?namespace=${encodeURIComponent(namespace)}`,
		{ headers: authHeaders },
	);
	if (!dcRes.ok)
		throw new Error(`list datacenters: ${dcRes.status} ${await dcRes.text()}`);
	const dc = (await dcRes.json()).datacenters[0]?.name;
	if (!dc) throw new Error("engine returned no datacenters");
	const res = await fetch(
		`${endpoint}/runner-configs/${encodeURIComponent(poolName)}?namespace=${encodeURIComponent(namespace)}`,
		{
			method: "PUT",
			headers: { ...authHeaders, "Content-Type": "application/json" },
			body: JSON.stringify({ datacenters: { [dc]: { normal: {} } } }),
		},
	);
	if (!res.ok)
		throw new Error(`upsert runner config: ${res.status} ${await res.text()}`);
}

async function waitForEnvoy(endpoint, namespace, token, poolName, timeoutMs) {
	const deadline = Date.now() + timeoutMs;
	const authHeaders = token ? { Authorization: `Bearer ${token}` } : {};
	while (Date.now() < deadline) {
		if (runtime && runtime.exitCode !== null) {
			throw new Error(`runtime exited before envoy:\n${childOutput()}`);
		}
		const r = await fetch(
			`${endpoint}/envoys?namespace=${encodeURIComponent(namespace)}&name=${encodeURIComponent(poolName)}`,
			{ headers: authHeaders },
		);
		if (r.ok && (await r.json()).envoys.length > 0) return;
		await new Promise((r) => setTimeout(r, 500));
	}
	throw new Error(`timed out waiting for envoy registration\n${childOutput()}`);
}

async function waitForActorReady(cb, timeoutMs) {
	const deadline = Date.now() + timeoutMs;
	let lastError;
	const retry =
		/(no_envoys|actor_ready_timeout|actor_wake_retries_exceeded|service_unavailable)/;
	while (Date.now() < deadline) {
		try {
			return await cb();
		} catch (error) {
			lastError = error;
			const message = error instanceof Error ? error.message : String(error);
			const code =
				error && typeof error === "object" && typeof error.code === "string"
					? error.code
					: "";
			if (!(retry.test(code) || retry.test(message))) throw error;
		}
		await new Promise((r) => setTimeout(r, 500));
	}
	throw lastError ?? new Error("timed out waiting for actor readiness");
}

async function stopRuntime(child) {
	if (child.exitCode !== null) return;
	child.kill("SIGINT");
	await new Promise((res) => {
		const t = setTimeout(() => {
			if (child.exitCode === null) child.kill("SIGKILL");
		}, 5000);
		child.once("exit", () => {
			clearTimeout(t);
			res();
		});
	});
}

async function main() {
	process.env.AGENTOS_SIDECAR_BIN = sidecarPath;
	process.env.AGENTOS_PLUGIN_BIN = pluginPath;
	process.env.RIVET_ENGINE_BINARY = engineBinary;

	const poolName = `agentos-demo-${crypto.randomUUID()}`;
	const namespace = "default";
	const token = "dev";
	// Fixed engine port (override with DEMO_ENGINE_PORT). Note: a stale engine
	// already bound here will make this run fail — `pkill -x rivet-engine` first.
	const enginePort = Number(process.env.DEMO_ENGINE_PORT ?? 6421);
	const endpoint = `http://127.0.0.1:${enginePort}`;
	let client;
	try {
		log(`starting runtime server on ${endpoint} (pool ${poolName})`);
		runtime = spawn(
			process.execPath,
			["--import", tsxLoaderPath, runtimeFixturePath],
			{
				cwd: r6RivetkitPackageRoot,
				env: {
					...process.env,
					RIVET_TOKEN: token,
					RIVET_NAMESPACE: namespace,
					RIVETKIT_TEST_ENDPOINT: endpoint,
					RIVETKIT_TEST_POOL_NAME: poolName,
					AGENTOS_TEST_SIDECAR_POOL: poolName,
					RIVET_RUN_ENGINE_HOST: "127.0.0.1",
					RIVET_RUN_ENGINE_PORT: String(enginePort),
					ESBK_TSCONFIG_PATH: join(r6RivetkitPackageRoot, "tsconfig.json"),
					TSX_TSCONFIG_PATH: join(r6RivetkitPackageRoot, "tsconfig.json"),
					RIVETKIT_STORAGE_PATH: mkdtempSync(join(tmpdir(), "agentos-demo-")),
				},
				stdio: ["ignore", "pipe", "pipe"],
			},
		);
		runtime.stdout?.on("data", (c) => {
			runtimeLogs.stdout += c.toString();
			if (process.env.DEMO_RUNTIME_LOG)
				appendFileSync(process.env.DEMO_RUNTIME_LOG, c);
		});
		runtime.stderr?.on("data", (c) => {
			runtimeLogs.stderr += c.toString();
			if (process.env.DEMO_RUNTIME_LOG)
				appendFileSync(process.env.DEMO_RUNTIME_LOG, c);
		});

		log("waiting for engine health...");
		await waitForHealth(endpoint, 90_000);
		log("upserting runner config...");
		await upsertNormalRunnerConfig(endpoint, namespace, token, poolName);
		log("waiting for envoy registration...");
		await waitForEnvoy(endpoint, namespace, token, poolName, 30_000);

		client = createClient({
			endpoint,
			token,
			namespace,
			poolName,
			disableMetadataLookup: true,
		});

		log("creating VM actor...");
		// Compute the actor key ONCE — otherwise each waitForActorReady retry
		// would create a fresh actor (new uuid), orphaning all but the last and
		// leaving the dashboard pointing at a cold, un-seeded actor.
		const actorKey = `agentos-demo-${crypto.randomUUID()}`;
		const handle = await waitForActorReady(() => client.os.create([actorKey]), 30_000);

		const path = "/tmp/agentos-demo.txt";
		const payload = "hello from a dummy agentOS+RivetKit example";
		log(`writeFile ${path}`);
		await waitForActorReady(() => handle.writeFile(path, payload), 30_000);
		log(`readFile ${path}`);
		const got = bytesToString(
			await waitForActorReady(() => handle.readFile(path), 30_000),
		);

		log(`read back: ${JSON.stringify(got)}`);
		if (got !== payload) throw new Error(`mismatch: ${got} !== ${payload}`);
		log("OK ✅ round-trip succeeded");

		// Seed a rich, deep filesystem + several processes so every VM-backed
		// inspector tab is well populated. Best-effort + resilient (allSettled).
		try {
			await Promise.allSettled([
				handle.mkdir("/root/project/src/lib"),
				handle.mkdir("/root/project/tests"),
				handle.mkdir("/root/logs"),
				handle.mkdir("/workspace/notes"),
			]);
			await Promise.allSettled([
				handle.writeFile("/root/README.md", "# agentOS demo VM\n\nSeeded by tests/fixtures/demo.mjs so the\ninspector tabs have live data to explore.\n\n- Filesystem: this tree\n- Processes: a few sleeps\n- Software/Mounts/Info: live config\n"),
				handle.writeFile("/root/data.json", JSON.stringify({ demo: true, items: [1, 2, 3], nested: { a: 1, b: [true, false] } }, null, 2)),
				handle.writeFile("/root/project/package.json", JSON.stringify({ name: "demo-project", version: "1.0.0", scripts: { build: "tsc" } }, null, 2)),
				handle.writeFile("/root/project/src/index.ts", "import { add } from './lib/math';\n\nexport function main(): void {\n\tconsole.log('sum', add(2, 3));\n}\n"),
				handle.writeFile("/root/project/src/lib/math.ts", "export const add = (a: number, b: number) => a + b;\nexport const mul = (a: number, b: number) => a * b;\n"),
				handle.writeFile("/root/project/tests/math.test.ts", "import { add } from '../src/lib/math';\n\ntest('add', () => expect(add(2, 3)).toBe(5));\n"),
				handle.writeFile("/root/project/main.py", "def main():\n\tprint('hello from python')\n\nif __name__ == '__main__':\n\tmain()\n"),
				handle.writeFile("/root/logs/app.log", Array.from({ length: 20 }, (_, i) => `[2026-06-29T17:0${i % 10}:00Z] INFO request ${i} handled in ${10 + i}ms`).join("\n") + "\n"),
				handle.writeFile("/workspace/notes/todo.md", "# TODO\n\n- [x] ship inspector tabs\n- [ ] sync the look\n"),
				// A binary file to exercise the viewer's binary-detection path.
				handle.writeFile("/root/blob.bin", new Uint8Array([0, 1, 2, 3, 255, 254, 0, 128, 7, 42, 0, 99])),
			]);
			if (!process.env.DEMO_NO_SLEEPS) {
				await Promise.allSettled([
					handle.spawn("sleep", ["86400"]),
					handle.spawn("sleep", ["7200"]),
					handle.spawn("sleep", ["1800"]),
					handle.spawn("sh", ["-c", "sleep 99999"]),
				]);
			}
			log("seeded demo filesystem + processes for the inspector tabs");
		} catch (e) {
			log(`(optional demo seeding skipped: ${e?.message ?? e})`);
		}

		// Optional: run a real pi agent session so the Transcript tab has live
		// data. Only when ANTHROPIC_API_KEY is set (the runtime-server then also
		// adds the pi software). The key is passed through the session env into
		// the VM and never written anywhere.
		if (process.env.ANTHROPIC_API_KEY) {
			try {
				log("creating pi agent session...");
				const { sessionId } = await waitForActorReady(
					() =>
						handle.createSession("pi", {
							env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY },
						}),
					60_000,
				);
				log(`pi session ${sessionId} created; sending prompt...`);
				const reply = await handle.sendPrompt(
					sessionId,
					"What is 2 + 2? Reply with just the number.",
				);
				log(`pi replied: ${JSON.stringify(reply).slice(0, 200)}`);
				log("→ open the Transcript tab to see this session");
			} catch (e) {
				log(`(pi session failed: ${e?.message ?? e})`);
				console.error("[demo] pi error object:", e);
				console.error("[demo] ===== runtime server output (stdout+stderr) =====");
				console.error(childOutput());
				console.error("[demo] ===== end runtime server output =====");
			}
		}

		// Keep the VM warm so the VM-backed inspector tabs (Processes/Filesystem)
		// always have a live VM to query. The actor otherwise hibernates on idle,
		// dropping the seeded process and cold-waking with action timeouts.
		setInterval(() => {
			handle.listProcesses?.().catch(() => {});
		}, 15_000);

		// [probe] readdirEntries null-on-not-found + session event/status shapes.
		if (process.env.DEMO_PROBE) {
			for (const p of ["/does-not-exist", "/tmp/agentos-demo.txt", "/root"]) {
				try {
					const r = await handle.readdirEntries(p);
					log(`[probe] readdirEntries ${p} -> ${r === null ? "null (not a dir)" : `array(${r.length})`}`);
				} catch (e) {
					log(`[probe] readdirEntries ${p} -> THREW ${e?.code ?? e?.name}: ${e?.message}`);
				}
			}
			try {
				const sessions = await handle.listPersistedSessions();
				log(`[probe] listPersistedSessions -> ${JSON.stringify(sessions)}`);
				const sid = sessions?.[0]?.sessionId;
				if (sid) {
					const evs = await handle.getSessionEvents(sid);
					const first = evs?.[0];
					log(`[probe] getSessionEvents(${sid}) -> ${evs?.length} events; keys(first)=${JSON.stringify(Object.keys(first ?? {}))}`);
					log(`[probe]   first.seq=${first?.seq} first.createdAt=${first?.createdAt} hasEvent=${first?.event != null}`);
				}
			} catch (e) {
				log(`[probe] sessions -> THREW ${e?.code ?? e?.name}: ${e?.message}`);
			}
			try {
				const sw = await handle.listSoftware();
				for (const s of sw ?? []) {
					const n = s.commands?.length ?? 0;
					log(`[probe] software ${s.package} (${s.kind}) -> ${n} cmds${n ? `: ${s.commands.slice(0, 8).join(", ")}${n > 8 ? ", …" : ""}` : ""}`);
				}
			} catch (e) {
				log(`[probe] listSoftware -> THREW ${e?.code ?? e?.name}: ${e?.message}`);
			}
		}

		// [hammer] Provoke the stateful sidecar spin by aggressively expanding
		// readdirEntries across the tree (mirrors inspector tree-expansion).
		// Fire-and-forget so the demo still reaches "staying up"; if a call
		// wedges the worker, the sidecar pegs a core and the watcher captures it.
		if (process.env.DEMO_HAMMER) {
			const expand = async (path, depth) => {
				if (depth < 0) return;
				let entries;
				try {
					entries = await handle.readdirEntries(path);
				} catch {
					return;
				}
				for (const e of entries) {
					if (e.isDirectory && !e.isSymbolicLink) {
						const child = path === "/" ? `/${e.name}` : `${path}/${e.name}`;
						await expand(child, depth - 1);
					}
				}
			};
			const roots = ["/", "/usr", "/lib", "/proc", "/sys", "/etc",
				"/__secure_exec", "/root", "/workspace", "/var", "/dev", "/host-tmp"];
			(async () => {
				for (let round = 1; round <= 200; round++) {
					log(`[hammer] round ${round}`);
					for (const root of roots) {
						await expand(root, 3).catch(() => {});
					}
				}
				log("[hammer] completed all rounds without wedging");
			})();
		}
	} catch (err) {
		await client?.dispose?.();
		if (runtime) await stopRuntime(runtime);
		throw err;
	}

	// Success: keep the engine + VM actor alive until the user interrupts.
	log("");
	log(`engine URL: ${endpoint}`);
	log(`health:     ${endpoint}/health`);
	log(`pool=${poolName} namespace=${namespace} token=${token}`);
	log("staying up — press Ctrl-C to stop");

	let stopping = false;
	const shutdown = async (sig) => {
		if (stopping) return;
		stopping = true;
		log(`received ${sig}, shutting down...`);
		await client?.dispose?.();
		if (runtime) await stopRuntime(runtime);
		process.exit(0);
	};
	process.on("SIGINT", () => void shutdown("SIGINT"));
	process.on("SIGTERM", () => void shutdown("SIGTERM"));
	await new Promise(() => {});
}

main().catch((err) => {
	console.error("[demo] FAILED:", err);
	process.exit(1);
});
