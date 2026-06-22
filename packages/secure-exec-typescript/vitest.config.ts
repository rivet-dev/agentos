import { resolve } from "node:path";

export default {
	resolve: {
		alias: [
			{
				find: "@rivet-dev/agentos-core/internal/runtime-compat",
				replacement: resolve(__dirname, "../core/dist/runtime-compat.js"),
			},
			{
				find: "@secure-exec/typescript",
				replacement: resolve(__dirname, "./src/index.ts"),
			},
			{
				find: "secure-exec",
				replacement: resolve(__dirname, "../secure-exec/dist/index.js"),
			},
		],
	},
	test: {
		testTimeout: 60_000,
	},
};
