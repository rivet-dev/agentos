import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
	resolve: {
		alias: [
			{
				find: "@rivet-dev/agentos-typescript",
				replacement: resolve(__dirname, "./src/index.ts"),
			},
		],
	},
	test: {
		testTimeout: 60_000,
	},
});
