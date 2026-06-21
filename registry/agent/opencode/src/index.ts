import { defineSoftware } from "@rivet-dev/agentos-core";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageDir = resolve(__dirname, "..");

const opencode = defineSoftware({
	name: "opencode",
	type: "agent" as const,
	packageDir,
	requires: ["@rivet-dev/agentos-opencode"],
	agent: {
		id: "opencode",
		// OpenCode still speaks ACP natively, but Agent OS runs a source-built
		// Node ACP bundle entirely inside the VM rather than a host binary wrapper.
		acpAdapter: "@rivet-dev/agentos-opencode",
		agentPackage: "@rivet-dev/agentos-opencode",
		staticEnv: {
			OPENCODE_DISABLE_CONFIG_DEP_INSTALL: "1",
			OPENCODE_DISABLE_EMBEDDED_WEB_UI: "1",
		},
	},
});

export default opencode;
