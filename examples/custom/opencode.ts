import opencode from "@agentos-software/opencode";

// OpenCode *is* the ACP process: it speaks ACP on stdio itself, so there is no
// separate adapter to spawn the agent. The published @agentos-software/opencode
// descriptor already encodes the right entrypoint and env, so use it as-is.
export default opencode;
