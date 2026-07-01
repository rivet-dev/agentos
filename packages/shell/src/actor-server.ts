// Runtime server for the shell's `--actor` mode — the shell-CLI equivalent of
// `packages/agentos/tests/fixtures/agentos-runtime-server.ts`. Boots the
// agentOS actor registry on the native runtime and serves it against a local
// rivet engine (spawned by the native registry via RIVET_RUN_ENGINE_PORT /
// RIVET_ENGINE_BINARY). Spawned as a child by `actor-vm.ts` with the shell's
// VM options passed through AGENTOS_SHELL_ACTOR_OPTIONS.

import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { agentOS } from "@rivet-dev/agentos";
import { allowAll } from "@rivet-dev/agentos-core/internal/runtime-compat";
import { setup } from "rivetkit";

const __dirname = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(__dirname, "../../..");
const r6Root =
	process.env.AGENTOS_R6_ROOT ?? resolve(workspaceRoot, "..", "r6");

const options = JSON.parse(process.env.AGENTOS_SHELL_ACTOR_OPTIONS ?? "{}");

const vm = agentOS({ permissions: allowAll, ...options });
const registry = setup({
	use: { vm },
	endpoint: process.env.AGENTOS_SHELL_ENDPOINT,
	namespace: process.env.RIVET_NAMESPACE ?? "default",
	token: process.env.RIVET_TOKEN ?? "dev",
	envoy: { poolName: process.env.AGENTOS_SHELL_POOL_NAME ?? "agentos-shell" },
	runtime: "native",
	shutdown: { disableSignalHandlers: true },
} as never);

// The native registry builder lives in the r6 rivetkit-typescript source tree
// (same import the actor e2e fixture uses); resolved dynamically so the shell
// package itself carries no static dependency on the sibling checkout.
const nativeModuleUrl = pathToFileURL(
	join(
		r6Root,
		"rivetkit-typescript",
		"packages",
		"rivetkit",
		"src",
		"registry",
		"native.ts",
	),
).href;
const { buildNativeRegistry } = await import(nativeModuleUrl);

const { registry: nativeRegistry, serveConfig } = await buildNativeRegistry(
	registry.parseConfig(),
);
if (process.env.RIVET_ENGINE_BINARY) {
	serveConfig.engineBinaryPath = process.env.RIVET_ENGINE_BINARY;
}

await nativeRegistry.serve(serveConfig);
