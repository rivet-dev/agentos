import { execFileSync, spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(packageRoot, "../..");
const storagePath = resolve(
	tmpdir(),
	`agentos-browser-terminal-e2e-${process.pid}`,
);
const pluginFilename =
	process.platform === "darwin"
		? "libagentos_actor_plugin.dylib"
		: process.platform === "win32"
			? "agentos_actor_plugin.dll"
			: "libagentos_actor_plugin.so";
const nativeEnv = {
	AGENTOS_PLUGIN_BIN: resolve(
		workspaceRoot,
		"target",
		"debug",
		pluginFilename,
	),
	AGENTOS_SIDECAR_BIN: resolve(
		workspaceRoot,
		"target",
		"debug",
		process.platform === "win32" ? "agentos-sidecar.exe" : "agentos-sidecar",
	),
};

async function waitFor(url, timeoutMs = 30_000) {
	const deadline = Date.now() + timeoutMs;
	let lastError;
	while (Date.now() < deadline) {
		try {
			const response = await fetch(url);
			if (response.ok) return;
			lastError = new Error(`health check returned HTTP ${response.status}`);
		} catch (error) {
			lastError = error;
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 100));
	}
	throw new Error(`timed out waiting for ${url}`, { cause: lastError });
}

function run(command, args, options = {}) {
	return spawn(command, args, {
		cwd: packageRoot,
		env: process.env,
		stdio: "inherit",
		...options,
	});
}

function listenerPids(port) {
	try {
		return execFileSync(
			"lsof",
			["-tiTCP:" + port, "-sTCP:LISTEN"],
			{ encoding: "utf8" },
		)
			.trim()
			.split("\n")
			.filter(Boolean)
			.map(Number);
	} catch (error) {
		if (error?.status !== 1) {
			console.error(`failed to inspect listener on port ${port}`, error);
		}
		return [];
	}
}

execFileSync("pnpm", ["prepare:browser-assets"], {
	cwd: packageRoot,
	stdio: "inherit",
});
execFileSync(
	"cargo",
	["build", "-p", "agentos-sidecar", "-p", "agentos-actor-plugin"],
	{
		cwd: workspaceRoot,
		stdio: "inherit",
	},
);

for (const port of [5173, 6420]) {
	if (listenerPids(port).length > 0) {
		throw new Error(`port ${port} is already in use; stop the demo before running E2E`);
	}
}

const dev = run("pnpm", ["dev:ready"], {
	detached: true,
	env: {
		...process.env,
		...nativeEnv,
		RIVETKIT_STORAGE_PATH: storagePath,
	},
});

let exitCode = 1;
try {
	await Promise.all([
		waitFor("http://127.0.0.1:5173"),
		waitFor("http://127.0.0.1:6420/health"),
	]);
	const test = run("pnpm", ["run", "test:e2e:connected"]);
	exitCode = await new Promise((resolveExit) => {
		test.once("exit", (code) => resolveExit(code ?? 1));
	});
} finally {
	try {
		process.kill(-dev.pid, "SIGTERM");
	} catch (error) {
		console.warn(`failed to terminate demo process group ${dev.pid}`, error);
	}
	await new Promise((resolveDelay) => setTimeout(resolveDelay, 250));
	try {
		process.kill(-dev.pid, "SIGKILL");
	} catch (error) {
		if (error?.code !== "ESRCH") {
			console.warn(`failed to reap demo process group ${dev.pid}`, error);
		}
	}
	// RivetKit intentionally detaches its local engine so it can outlive the
	// registry process. These ports were verified free above, so any listener
	// here belongs to this E2E invocation and must be torn down with it.
	for (const pid of listenerPids(6420)) {
		try {
			process.kill(pid, "SIGTERM");
		} catch (error) {
			console.warn(`failed to terminate detached engine ${pid}`, error);
		}
	}
}

process.exitCode = exitCode;
