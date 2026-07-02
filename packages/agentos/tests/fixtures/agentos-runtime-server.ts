import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setup } from "rivetkit";
import { agentOs } from "../../src/index.js";
import { buildNativeRegistry } from "../../../../../r6/rivetkit-typescript/packages/rivetkit/src/registry/native";

// Load the pi agent package ONLY when an Anthropic key is present. Import it
// lazily (not a static top-level import) so the demo and CI boot without
// requiring @agentos-software/pi's `dist/` to be built when no key is set.
const pi = process.env.ANTHROPIC_API_KEY
	? (await import("@agentos-software/pi")).default
	: undefined;

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
				// Add the pi agent ONLY when an Anthropic key is present (i.e. the
				// inspector demo run) so the Transcript tab can show a real session;
				// CI/test runs have no key and stay agent-free.
				software: pi ? [pi] : [],
				// Demo mounts so the "Mounts" inspector tab is richly populated
				// (declarative config; memory + a read-only host_dir, no host risk).
				mounts: [
					{ path: "/scratch", plugin: { id: "memory", config: {} } },
					{ path: "/cache", plugin: { id: "memory", config: {} } },
					{ path: "/data", plugin: { id: "memory", config: {} } },
					{
						path: "/host-tmp",
						plugin: {
							id: "host_dir",
							config: { hostPath: "/tmp/host-tmp", readOnly: true },
						},
						readOnly: true,
					},
				],
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
