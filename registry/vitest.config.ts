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
		testTimeout: 30000,
		hookTimeout: 30000,
	},
};
