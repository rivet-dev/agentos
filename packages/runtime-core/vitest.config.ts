import { defineConfig } from "vitest/config";

export default defineConfig({
	test: {
		include: ["tests/**/*.test.ts"],
		// Integration tests start debug sidecars and warm V8 isolates. A 5s
		// per-test default is below normal loaded-runner latency and causes random
		// failures across otherwise unrelated process and network tests.
		testTimeout: 30_000,
		// Many test files each spawn a debug sidecar + V8 warm isolates;
		// running files in parallel thrashes small CI runners until frame
		// waits exceed their 120s timeout (mount-fs-custom-vfs and
		// node-runtime-exec-output timed out deterministically on 4-core
		// GitHub runners, and passed serially). Keep files sequential.
		fileParallelism: false,
	},
});
