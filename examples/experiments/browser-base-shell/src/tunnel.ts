import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const stateKey = createHash("sha256")
	.update(packageRoot)
	.digest("hex")
	.slice(0, 12);
const stateDir = join(tmpdir(), `agentos-browser-base-shell-${stateKey}`);
const statePath = join(stateDir, "cloudflared.json");
const logPath = join(stateDir, "cloudflared.log");
const daemonPath = fileURLToPath(
	new URL("./tunnel-daemon.mjs", import.meta.url),
);

export interface PersistentTunnelState {
	version: 1;
	supervisorPid: number;
	cloudflaredPid: number;
	port: number;
	url: string;
	startedAt: string;
}

interface EnsurePersistentTunnelOptions {
	port: number;
	onStatus?: (message: string) => void;
	validate: (url: string) => Promise<void>;
}

function delay(ms: number): Promise<void> {
	return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

function processIsRunning(pid: number): boolean {
	if (!Number.isSafeInteger(pid) || pid <= 0) return false;
	try {
		process.kill(pid, 0);
		return true;
	} catch (error) {
		return (error as NodeJS.ErrnoException).code === "EPERM";
	}
}

function readState(): PersistentTunnelState | undefined {
	try {
		const value = JSON.parse(
			readFileSync(statePath, "utf8"),
		) as Partial<PersistentTunnelState>;
		if (
			value.version !== 1 ||
			!Number.isSafeInteger(value.supervisorPid) ||
			!Number.isSafeInteger(value.cloudflaredPid) ||
			!Number.isSafeInteger(value.port) ||
			typeof value.url !== "string" ||
			!/^https:\/\/[a-z0-9-]+\.trycloudflare\.com$/i.test(value.url) ||
			typeof value.startedAt !== "string"
		) {
			return undefined;
		}
		return value as PersistentTunnelState;
	} catch {
		return undefined;
	}
}

function diagnosticLog(): string {
	try {
		return readFileSync(logPath, "utf8");
	} catch {
		return "cloudflared produced no diagnostic log";
	}
}

async function terminateProcessGroup(pid: number): Promise<void> {
	if (!processIsRunning(pid)) return;
	try {
		process.kill(-pid, "SIGTERM");
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
		return;
	}
	const deadline = Date.now() + 2_000;
	while (Date.now() < deadline && processIsRunning(pid)) await delay(50);
	if (!processIsRunning(pid)) return;
	try {
		process.kill(-pid, "SIGKILL");
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
	}
}

async function clearPersistentTunnel(
	state?: PersistentTunnelState,
): Promise<void> {
	if (state) await terminateProcessGroup(state.supervisorPid);
	rmSync(statePath, { force: true });
}

async function startTunnelDaemon(port: number): Promise<PersistentTunnelState> {
	mkdirSync(stateDir, { recursive: true });
	rmSync(statePath, { force: true });
	const daemon = spawn(
		process.execPath,
		[daemonPath, String(port), statePath, logPath],
		{
			cwd: packageRoot,
			detached: true,
			stdio: "ignore",
		},
	);
	if (!daemon.pid)
		throw new Error("failed to start persistent cloudflared supervisor");
	daemon.unref();

	const deadline = Date.now() + 30_000;
	while (Date.now() < deadline) {
		const state = readState();
		if (state?.supervisorPid === daemon.pid && processIsRunning(daemon.pid)) {
			return state;
		}
		if (!processIsRunning(daemon.pid)) {
			throw new Error(
				`cloudflared supervisor exited before startup:\n${diagnosticLog()}`,
			);
		}
		await delay(100);
	}
	await terminateProcessGroup(daemon.pid);
	throw new Error(
		`timed out waiting for persistent cloudflared:\n${diagnosticLog()}`,
	);
}

export async function ensurePersistentTunnel(
	options: EnsurePersistentTunnelOptions,
): Promise<PersistentTunnelState> {
	const report = options.onStatus ?? (() => undefined);
	let state = readState();
	if (
		state &&
		(!processIsRunning(state.supervisorPid) || state.port !== options.port)
	) {
		await clearPersistentTunnel(state);
		state = undefined;
	}
	if (state) {
		try {
			await options.validate(state.url);
			report(`reusing persistent Cloudflare Quick Tunnel ${state.url}`);
			return state;
		} catch {
			report("persistent Cloudflare Quick Tunnel is unhealthy; restarting it");
			await clearPersistentTunnel(state);
			state = undefined;
		}
	}

	let lastError: unknown;
	for (let attempt = 1; attempt <= 3; attempt += 1) {
		report(
			`starting persistent Cloudflare Quick Tunnel (attempt ${attempt}/3)`,
		);
		try {
			state = await startTunnelDaemon(options.port);
			await options.validate(state.url);
			return state;
		} catch (error) {
			lastError = error;
			await clearPersistentTunnel(state);
			state = undefined;
		}
	}
	throw new Error("failed to start persistent Cloudflare Quick Tunnel", {
		cause: lastError,
	});
}

export function persistentTunnelStatus(): PersistentTunnelState | undefined {
	const state = readState();
	return state && processIsRunning(state.supervisorPid) ? state : undefined;
}

export async function stopPersistentTunnel(): Promise<boolean> {
	const state = readState();
	if (!state) {
		rmSync(statePath, { force: true });
		return false;
	}
	await clearPersistentTunnel(state);
	return true;
}

export const persistentTunnelPaths = { statePath, logPath };
