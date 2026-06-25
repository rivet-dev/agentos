import { defineSoftware } from "@rivet-dev/agentos";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// Example: an adapter to run OpenCode.
export default defineSoftware({
  name: "opencode",
  type: "agent",
  packageDir,
  // A single package provides everything that runs in the VM.
  requires: ["@agentos-software/opencode"],
  agent: {
    id: "opencode",
    // Same package for both: OpenCode *is* the ACP process. It speaks ACP
    // on stdio itself, so there is no separate adapter to spawn the agent.
    acpAdapter: "@agentos-software/opencode",
    agentPackage: "@agentos-software/opencode",
    staticEnv: {
      OPENCODE_DISABLE_CONFIG_DEP_INSTALL: "1",
      OPENCODE_DISABLE_EMBEDDED_WEB_UI: "1",
    },
  },
});
