import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterAll } from "vitest";
import { __disposeAllSharedSidecarsForTesting } from "../../src/agent-os.js";

// Runtime resolution never probes repository build output. The repository test
// harness opts into its explicit sidecar artifact without changing VM config.
if (!process.env.AGENTOS_SIDECAR_BIN) {
	const testSidecar = fileURLToPath(
		new URL("../../../../target/debug/agentos-sidecar", import.meta.url),
	);
	if (existsSync(testSidecar)) {
		process.env.AGENTOS_SIDECAR_BIN = testSidecar;
	}
}

// Vitest forks a worker per file. Each worker holds the process-global
// `sharedSidecars` map, so we must dispose the shared sidecar on file teardown
// or the underlying native sidecar subprocess keeps its piped stdio open and
// blocks the worker (and therefore `pnpm test`) from exiting.
afterAll(async () => {
	await __disposeAllSharedSidecarsForTesting();
});
