import { defineConfig } from "tsup";

export default defineConfig({
	format: ["esm"],
	dts: true,
	sourcemap: true,
	clean: true,
	// Keep rivetkit / react out of the bundle so subpath conditions
	// (browser client, react) resolve in the consumer's environment.
	external: ["rivetkit", "@rivetkit/react", "react", "react-dom"],
});
