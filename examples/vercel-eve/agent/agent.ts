import { defineAgent } from "eve";

export default defineAgent({
	model: "openai/gpt-5.4-mini",
	build: {
		externalDependencies: [
			"@rivet-dev/agentos",
			"@rivet-dev/agentos-core",
			"@rivet-dev/agentos-eve",
			"@rivet-dev/agentos-runtime-core",
			"@rivet-dev/agentos-sidecar",
			"@rivetkit/engine-cli",
			"@rivetkit/engine-cli-linux-x64-musl",
		],
	},
});
