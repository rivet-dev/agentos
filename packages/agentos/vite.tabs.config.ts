import { createRequire } from "node:module";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const require = createRequire(import.meta.url);

// Builds the shared inspector-tab app to a single static asset dir that ships
// with the package (declared in package.json `files`). `base: "./"` keeps asset
// URLs relative so they resolve under any `/inspector/custom-tabs/<id>/` path.
export default defineConfig({
	root: "src/inspector-tabs",
	base: "./",
	resolve: {
		alias: {
			// @xterm/headless@6.0.0 ships only the CJS build but its `module`
			// field points at a `lib/` ESM file missing from the tarball; point
			// straight at the real entry (`main`) so vite can resolve it.
			"@xterm/headless": require.resolve("@xterm/headless"),
		},
	},
	build: {
		outDir: "../../assets/inspector-tabs-app",
		emptyOutDir: true,
		sourcemap: false,
	},
	plugins: [react()],
});
