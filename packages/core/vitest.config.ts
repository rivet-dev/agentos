import { configDefaults, defineConfig } from "vitest/config";

export default defineConfig({
	test: {
		// Native runtime tests share heavyweight process resources and the V8
		// platform, so keep file execution serialized.
		fileParallelism: false,
		hookTimeout: 30000,
		setupFiles: ["tests/helpers/default-vm-permissions.ts"],
		testTimeout: 30000,
		include: ["tests/**/*.test.ts"],
		exclude: configDefaults.exclude,
	},
});
