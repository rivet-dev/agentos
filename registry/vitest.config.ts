import { readdirSync } from "node:fs";
import { resolve } from "node:path";

const pnpmStoreDir = resolve(import.meta.dirname, "..", "node_modules", ".pnpm");
const xtermHeadlessPackageDir = readdirSync(pnpmStoreDir).find((entry) =>
	entry.startsWith("@xterm+headless@"),
);

if (!xtermHeadlessPackageDir) {
	throw new Error(`Could not find @xterm/headless in ${pnpmStoreDir}`);
}

export default {
	resolve: {
		alias: {
			"@xterm/headless": resolve(
				pnpmStoreDir,
				xtermHeadlessPackageDir,
				"node_modules",
				"@xterm",
				"headless",
			),
		},
	},
	test: {
		exclude: [
			"**/node_modules/**",
			"**/dist/**",
			"**/cypress/**",
			"**/.{idea,git,cache,output,temp}/**",
			"**/{karma,rollup,webpack,vite,vitest,jest,ava,babel,nyc,cypress,tsup,build,eslint,prettier}.config.*",
			"agent/*/tests/*.test.mjs",
		],
		testTimeout: 30000,
		hookTimeout: 30000,
	},
};
