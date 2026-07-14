import { defineConfig } from "vitest/config";

export default defineConfig({
	test: {
		include: ["tests/**/*.test.ts"],
		// Many test files each spawn a debug sidecar + V8 warm isolates;
		// running files in parallel thrashes small CI runners until frame
		// waits exceed their 120s timeout. Keep files sequential.
		fileParallelism: false,
	},
});
