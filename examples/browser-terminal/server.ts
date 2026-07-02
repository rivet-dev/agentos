// Browser Terminal — RivetKit server.
//
// A single custom RivetKit actor ("shellVm") owns one Agent OS VM. Each actor
// instance (keyed by the id kept in the browser's localStorage) is an isolated
// VM. The actor exposes PTY-style shell actions; the browser writes keystrokes
// with `writeShell` and pulls output with `readShell` (an incremental cursor),
// so the whole terminal lives in the browser over RivetKit — no bespoke
// WebSocket server.
//
// Why polling instead of broadcasting output as events: in this local
// native-registry setup RivetKit only flushes queued broadcasts to a connection
// when some *other* connection is active, so a lone browser driving its own
// shell never receives its own output. Request/response actions are reliable, so
// output rides on `readShell`.
//
// Why a hand-written actor instead of the shipped `agentOS()` actor: the browser
// needs an interactive PTY channel (open / write / resize / read / reconnect).
// This actor implements exactly that on top of the public
// `@rivet-dev/agentos-core` `AgentOs` shell API (`openShell`, `onShellData`,
// `writeShell`, `resizeShell`, `closeShell`). The VM handle is a
// non-serializable runtime resource, so it lives in the actor's `vars`.

import { join } from "node:path";
import { pathToFileURL } from "node:url";
import common from "@agentos-software/common";
import everything from "@agentos-software/everything";
import git from "@agentos-software/git";
import httpGet from "@agentos-software/http-get";
import sqlite3 from "@agentos-software/sqlite3";
import { AgentOs } from "@rivet-dev/agentos-core";
// `setup` from @rivet-dev/agentos raises the RivetKit transport message-size
// caps to never-hit-by-normal-use values (terminal bursts stream as single
// messages).
import { setup } from "@rivet-dev/agentos";
import { actor } from "rivetkit";

/** Per-shell scrollback kept so a reconnecting browser can replay history. */
const MAX_SCROLLBACK_BYTES = 256 * 1024;

// One hour: far past any real interaction, but still finite (never Infinity)
// per the limits/observability policy. The stock RivetKit defaults (5s connect,
// 60s action, 30s sleep) would reap a live terminal mid-session.
const NEVER_HIT_MS = 60 * 60 * 1000;

interface ShellRecord {
	unsub: () => void;
	/** Bounded scrollback ring (raw PTY bytes). */
	chunks: Uint8Array[];
	/** Bytes currently held in `chunks`. */
	size: number;
	/** Monotonic count of bytes ever emitted (the read cursor space). */
	emitted: number;
	title: string;
	createdAt: number;
}

interface Vars {
	/** Lazily created on the first action — see `ensureVm`. */
	vm: AgentOs | null;
	vmPromise: Promise<AgentOs> | null;
	shells: Map<string, ShellRecord>;
}

interface OpenShellArgs {
	cols?: number;
	rows?: number;
	cwd?: string;
	title?: string;
}

const encodeBytes = (data: Uint8Array): string =>
	Buffer.from(data).toString("base64");

function pushOutput(rec: ShellRecord, data: Uint8Array): void {
	rec.chunks.push(data);
	rec.size += data.length;
	rec.emitted += data.length;
	while (rec.size > MAX_SCROLLBACK_BYTES && rec.chunks.length > 1) {
		const dropped = rec.chunks.shift();
		if (dropped) rec.size -= dropped.length;
	}
}

/**
 * Return the output bytes emitted after `fromOffset`, plus the new cursor.
 * Clients poll this to render a shell (echoes + command output) — RivetKit's
 * broadcast delivery stalls for a lone connection driving its own actor, so
 * output rides on this request/response instead of events.
 */
function readSince(
	rec: ShellRecord,
	fromOffset: number,
): { offset: number; data: string } {
	const bufStart = rec.emitted - rec.size;
	const start = Math.max(0, fromOffset - bufStart);
	const buf = Buffer.concat(rec.chunks.map((c) => Buffer.from(c)));
	return {
		offset: rec.emitted,
		data: start >= buf.length ? "" : encodeBytes(buf.subarray(start)),
	};
}

// Booting a VM (spawning the sidecar) takes a few seconds, which is longer than
// RivetKit's actor-ready connection guard. So the VM is created lazily on the
// first action (inside the high `actionTimeout` window) rather than in
// `createVars`, letting the actor become ready — and the browser connect —
// immediately. `common` provides `sh` + coreutils (WASM commands).
function ensureVm(vars: Vars): Promise<AgentOs> {
	if (vars.vm) return Promise.resolve(vars.vm);
	if (!vars.vmPromise) {
		const t0 = Date.now();
		console.error("[shellVm] booting VM…");
		vars.vmPromise = AgentOs.create({
			// `common` provides `sh`; `everything` + git/http-get/sqlite3 match the
			// tool set the agentos shell ships (git, curl, ripgrep, grep, sed, jq,
			// sqlite3, …). There is no vim/editor package in Agent OS, so none is
			// included.
			software: [common, everything, git, httpGet, sqlite3],
		}).then((vm) => {
			vars.vm = vm;
			console.error(`[shellVm] VM ready in ${Date.now() - t0}ms`);
			return vm;
		});
		vars.vmPromise.catch((e) =>
			console.error("[shellVm] VM boot failed:", e),
		);
	}
	return vars.vmPromise;
}

const shellVm = actor({
	options: {
		// Keep the VM (and therefore its shells) alive so a browser can
		// reconnect to running terminal sessions.
		noSleep: true,
		createVarsTimeout: NEVER_HIT_MS,
		onConnectTimeout: NEVER_HIT_MS,
		onBeforeConnectTimeout: NEVER_HIT_MS,
		actionTimeout: NEVER_HIT_MS,
		connectionLivenessTimeout: NEVER_HIT_MS,
		sleepTimeout: NEVER_HIT_MS,
		maxQueueMessageSize: 512 * 1024 * 1024,
	},

	// Runtime resources (the VM handle) are not serializable, so they live in
	// vars, not state. The VM itself is created lazily (see `ensureVm`).
	createVars: async (): Promise<Vars> => {
		return { vm: null, vmPromise: null, shells: new Map() };
	},

	onDestroy: async (c) => {
		const vars = c.vars as Vars;
		for (const rec of vars.shells.values()) rec.unsub();
		vars.shells.clear();
		try {
			await vars.vm?.dispose();
		} catch {
			// best-effort teardown
		}
	},

	actions: {
		/** Open a new PTY-style shell; returns its id. Read output via readShell. */
		openShell: async (c, args?: OpenShellArgs) => {
			const vars = c.vars as Vars;
			const vm = await ensureVm(vars);
			const { shellId } = vm.openShell({
				cols: args?.cols ?? 80,
				rows: args?.rows ?? 24,
				cwd: args?.cwd,
			});
			const rec: ShellRecord = {
				unsub: () => {},
				chunks: [],
				size: 0,
				emitted: 0,
				title: args?.title ?? "shell",
				createdAt: Date.now(),
			};
			rec.unsub = vm.onShellData(shellId, (data) => pushOutput(rec, data));
			vars.shells.set(shellId, rec);
			return { shellId };
		},

		/** Forward keystrokes to a shell's PTY input. */
		writeShell: async (c, shellId: string, data: string) => {
			const vars = c.vars as Vars;
			// Fail loudly on an unknown shell (e.g. a stale browser tab pointing at
			// a shell from a previous VM) instead of silently dropping input.
			if (!vars.vm || !vars.shells.has(shellId)) {
				throw new Error(`shell not found: ${shellId}`);
			}
			vars.vm.writeShell(shellId, data);
		},

		/** Notify a shell of a terminal resize. */
		resizeShell: async (c, shellId: string, cols: number, rows: number) => {
			const vars = c.vars as Vars;
			if (!vars.vm || !vars.shells.has(shellId)) return;
			vars.vm.resizeShell(shellId, cols, rows);
		},

		/** Kill a shell and drop it from tracking. */
		closeShell: async (c, shellId: string) => {
			const vars = c.vars as Vars;
			const rec = vars.shells.get(shellId);
			if (rec) rec.unsub();
			try {
				vars.vm?.closeShell(shellId);
			} catch {
				// already gone
			}
			vars.shells.delete(shellId);
		},

		/** List currently-open shells (for reconnecting browsers). */
		listShells: async (c) => {
			const vars = c.vars as Vars;
			return [...vars.shells.entries()].map(([shellId, rec]) => ({
				shellId,
				title: rec.title,
				createdAt: rec.createdAt,
			}));
		},

		/**
		 * Pull shell output emitted after `fromOffset`. Clients poll this to
		 * render both the reconnect scrollback (fromOffset 0) and live output.
		 * `gone: true` means the shell no longer exists (drop the tab).
		 */
		readShell: async (c, shellId: string, fromOffset: number) => {
			const vars = c.vars as Vars;
			const rec = vars.shells.get(shellId);
			if (!rec) return { gone: true, offset: fromOffset, data: "" };
			return { gone: false, ...readSince(rec, fromOffset ?? 0) };
		},
	},
});

// ---------------------------------------------------------------------------
// Local run: RivetKit native engine + slotted actor-host envoy
// ---------------------------------------------------------------------------
// RivetKit hosts the actor on an "envoy" scheduled by a local Rivet engine.
// `buildNativeRegistry(...).serve(...)` spawns the engine and registers a
// slotted envoy that can host this process's actors (the published
// `registry.start()` only registers a zero-slot manager envoy, so the engine
// reports `no_envoys`). After serving we upsert a runner-config for the pool and
// wait for the envoy to register; then the browser can talk to the actor via
// the engine endpoint. This bootstrap mirrors packages/shell's actor mode +
// packages/agentos actor tests; it needs the sibling `r6` rivetkit checkout for
// the native registry builder (run via `npm run server`, which wires the loader
// + engine binary).
const NAMESPACE = process.env.RIVET_NAMESPACE ?? "default";
const TOKEN = process.env.RIVET_TOKEN ?? "dev";
const POOL = process.env.RIVET_POOL ?? "default";
const ENGINE_HOST = process.env.RIVET_RUN_ENGINE_HOST ?? "127.0.0.1";
const ENGINE_PORT = Number(process.env.RIVET_RUN_ENGINE_PORT ?? 6642);
const ENGINE_ENDPOINT = `http://${ENGINE_HOST}:${ENGINE_PORT}`;
const auth = { Authorization: `Bearer ${TOKEN}` };

export const registry = setup({
	use: { shellVm },
	endpoint: ENGINE_ENDPOINT,
	namespace: NAMESPACE,
	token: TOKEN,
	envoy: { poolName: POOL },
	runtime: "native",
	shutdown: { disableSignalHandlers: true },
} as never);

async function waitForEngineHealth(timeoutMs: number): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		try {
			const r = await fetch(`${ENGINE_ENDPOINT}/health`);
			if (r.ok) return;
		} catch {
			// not up yet
		}
		await new Promise((r) => setTimeout(r, 300));
	}
	throw new Error(`engine not healthy at ${ENGINE_ENDPOINT}`);
}

// The engine creates the `default` namespace asynchronously on startup, so the
// datacenter list + runner-config PUT can briefly 400 with "namespace does not
// exist". Retry until the namespace is ready and the upsert succeeds.
async function upsertRunnerConfig(timeoutMs: number): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let lastErr = "unknown";
	while (Date.now() < deadline) {
		try {
			const dcRes = await fetch(
				`${ENGINE_ENDPOINT}/datacenters?namespace=${NAMESPACE}`,
				{ headers: auth },
			);
			if (dcRes.ok) {
				const { datacenters } = (await dcRes.json()) as {
					datacenters: Array<{ name: string }>;
				};
				const dc = datacenters[0]?.name;
				if (dc) {
					const res = await fetch(
						`${ENGINE_ENDPOINT}/runner-configs/${POOL}?namespace=${NAMESPACE}`,
						{
							method: "PUT",
							headers: { ...auth, "Content-Type": "application/json" },
							body: JSON.stringify({ datacenters: { [dc]: { normal: {} } } }),
						},
					);
					if (res.ok) return;
					lastErr = `upsert ${res.status}: ${await res.text()}`;
				} else {
					lastErr = "no datacenters yet";
				}
			} else {
				lastErr = `datacenters ${dcRes.status}`;
			}
		} catch (e) {
			lastErr = String(e);
		}
		await new Promise((r) => setTimeout(r, 500));
	}
	throw new Error(`runner config not ready in time: ${lastErr}`);
}

async function waitForEnvoy(timeoutMs: number): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	while (Date.now() < deadline) {
		const r = await fetch(
			`${ENGINE_ENDPOINT}/envoys?namespace=${NAMESPACE}&name=${POOL}`,
			{ headers: auth },
		);
		if (r.ok) {
			const { envoys } = (await r.json()) as { envoys: unknown[] };
			if (envoys.length > 0) return;
		}
		await new Promise((r) => setTimeout(r, 300));
	}
	throw new Error("timed out waiting for envoy registration");
}

// The native registry builder lives in the sibling r6 rivetkit checkout (TS
// source using `@/` aliases), so this file must run under that checkout's tsx
// loader/tsconfig — `npm run server` handles it.
const r6Root =
	process.env.AGENTOS_R6_ROOT ?? "/home/nathan/.herdr/workspaces/agent-os/r6";
const nativeUrl = pathToFileURL(
	join(
		r6Root,
		"rivetkit-typescript/packages/rivetkit/src/registry/native.ts",
	),
).href;
const { buildNativeRegistry } = await import(nativeUrl);
const { registry: nativeRegistry, serveConfig } = await buildNativeRegistry(
	(registry as unknown as { parseConfig: () => unknown }).parseConfig(),
);
if (process.env.RIVET_ENGINE_BINARY) {
	serveConfig.engineBinaryPath = process.env.RIVET_ENGINE_BINARY;
}
await nativeRegistry.serve(serveConfig);

await waitForEngineHealth(90_000);
await upsertRunnerConfig(60_000);
await waitForEnvoy(30_000);
console.error(
	`[shellVm] ready — engine ${ENGINE_ENDPOINT}, namespace=${NAMESPACE}, pool=${POOL}`,
);
