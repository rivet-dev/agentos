import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The fixture imports the built package entry (dist), like a consumer would.
// `pnpm test` builds packages/core first; when running this file standalone
// without a build, skip with a clear reason instead of a confusing import error.
const DIST_ENTRY = resolve(import.meta.dirname, "../dist/index.js");
const distMissing = !existsSync(DIST_ENTRY);
if (distMissing) {
	// eslint-disable-next-line no-console
	console.warn(
		`[shared-sidecar-clean-exit] skipped: build packages/core first (missing ${DIST_ENTRY})`,
	);
}

/**
 * REGRESSION: a standalone script that creates a VM and calls `await
 * vm.dispose()` must let the node process exit on its own.
 *
 * `AgentOs.create()` uses the process-global SHARED sidecar pool. `vm.dispose()`
 * releases the VM lease, but the shared sidecar's child process + stdio sockets
 * used to stay referenced, keeping the event loop alive forever — every
 * one-shot quickstart script (hello-world, filesystem, cron, agent-session…)
 * hung on exit and had to be SIGINT'd. The fix unrefs the shared sidecar's
 * handles when no leases are active (re-refs on the next lease), so the loop can
 * drain. This runs the script as a real subprocess and asserts it exits by
 * itself, with no `process.exit()` escape hatch.
 */
describe("shared sidecar clean exit", () => {
	it.skipIf(distMissing)("a standalone create()+dispose() script exits on its own", () => {
		const script = resolve(
			import.meta.dirname,
			"fixtures/shared-sidecar-clean-exit-script.mjs",
		);
		const result = spawnSync(process.execPath, [script], {
			cwd: resolve(import.meta.dirname, ".."),
			encoding: "utf8",
			timeout: 60_000,
		});

		const diag = `exit=${result.status} signal=${result.signal}\nstdout: ${result.stdout ?? ""}\nstderr: ${(result.stderr ?? "").slice(-800)}`;

		// The script logic should complete regardless of the exit behavior.
		expect(result.stdout ?? "", `script never finished its work.\n${diag}`).toContain(
			"SCRIPT_DONE",
		);
		// The real assertion: the process terminated on its own (was not killed
		// by the spawn timeout). A hang leaves signal === "SIGTERM".
		expect(
			result.signal,
			`process did not exit on its own within 60s — the shared sidecar kept the event loop alive.\n${diag}`,
		).toBeNull();
		expect(result.status, `non-zero exit.\n${diag}`).toBe(0);
	}, 90_000);
});
