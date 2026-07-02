// Launches server.ts under the sibling r6 rivetkit checkout's tsx loader +
// tsconfig, because the native registry builder it imports is TS source that
// uses `@/` path aliases. Also resolves the local Rivet engine binary and picks
// the engine port. This mirrors how packages/shell runs its actor-mode VM.
//
// Override the r6 checkout with AGENTOS_R6_ROOT and the port with PORT.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

const r6Root =
	process.env.AGENTOS_R6_ROOT ?? "/home/nathan/.herdr/workspaces/agent-os/r6";
const r6Rk = join(r6Root, "rivetkit-typescript", "packages", "rivetkit");
const tsxLoader = join(r6Rk, "node_modules", "tsx", "dist", "loader.mjs");
const r6Tsconfig = join(r6Rk, "tsconfig.json");

if (!existsSync(tsxLoader)) {
	console.error(
		`Cannot find the r6 rivetkit tsx loader at ${tsxLoader}.\n` +
			"This example needs the sibling `r6` rivetkit checkout to host the actor " +
			"(the native registry builder is TS source there). Set AGENTOS_R6_ROOT.",
	);
	process.exit(1);
}

let engineBinary = process.env.RIVET_ENGINE_BINARY;
if (!engineBinary) {
	try {
		const pkg = require.resolve(
			"@rivetkit/engine-cli-linux-x64-musl/package.json",
		);
		const candidate = join(dirname(pkg), "rivet-engine");
		if (existsSync(candidate)) engineBinary = candidate;
	} catch {
		// platform package not installed; serve() will report binary_unavailable
	}
}

const port = process.env.PORT ?? "6642";

const child = spawn(
	process.execPath,
	["--import", tsxLoader, join(here, "server.ts")],
	{
		cwd: r6Rk,
		stdio: "inherit",
		env: {
			...process.env,
			ESBK_TSCONFIG_PATH: r6Tsconfig,
			TSX_TSCONFIG_PATH: r6Tsconfig,
			RIVET_RUN_ENGINE_HOST: "127.0.0.1",
			RIVET_RUN_ENGINE_PORT: port,
			RIVET_TOKEN: process.env.RIVET_TOKEN ?? "dev",
			RIVET_NAMESPACE: process.env.RIVET_NAMESPACE ?? "default",
			AGENTOS_R6_ROOT: r6Root,
			...(engineBinary ? { RIVET_ENGINE_BINARY: engineBinary } : {}),
			RIVETKIT_STORAGE_PATH:
				process.env.RIVETKIT_STORAGE_PATH ??
				mkdtempSync(join(tmpdir(), "browser-terminal-")),
		},
	},
);
child.on("exit", (code) => process.exit(code ?? 0));
