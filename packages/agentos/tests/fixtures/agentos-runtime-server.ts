import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setup } from "rivetkit";
import { agentOs } from "../../src/index.js";
import { buildNativeRegistry } from "../../../../../r6/rivetkit-typescript/packages/rivetkit/src/registry/native";

const fixtureDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(fixtureDir, "../../../..");
const r6Root = resolve(repoRoot, "../r6");
const repoEngineBinary = join(r6Root, "target/debug/rivet-engine");

function resolveEngineBinaryPath(): string | undefined {
	if (existsSync(repoEngineBinary)) return repoEngineBinary;
	return process.env.RIVET_ENGINE_BINARY;
}

const registry = setup({
	use: {
		os: agentOs({
			options: {
				permissions: {
					fs: "allow",
					network: "allow",
					childProcess: "allow",
					process: "allow",
					env: "allow",
				},
				sidecar: {
					kind: "shared",
					pool: process.env.AGENTOS_TEST_SIDECAR_POOL,
				},
			},
		}),
	},
	endpoint: process.env.RIVETKIT_TEST_ENDPOINT ?? "http://127.0.0.1:6642",
	namespace: process.env.RIVET_NAMESPACE ?? "default",
	token: process.env.RIVET_TOKEN ?? "dev",
	envoy: {
		poolName: process.env.RIVETKIT_TEST_POOL_NAME ?? "agentos-package",
	},
	runtime: "native",
	shutdown: { disableSignalHandlers: true },
});

const { registry: nativeRegistry, serveConfig } = await buildNativeRegistry(
	registry.parseConfig(),
);
const engineBinaryPath = resolveEngineBinaryPath();
if (engineBinaryPath) serveConfig.engineBinaryPath = engineBinaryPath;

await nativeRegistry.serve(serveConfig);
