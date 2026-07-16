#!/usr/bin/env node

const prompt = process.env.ACP_APPEND_SYSTEM_PROMPT;
if (prompt) {
	process.argv.push("--append-system-prompt", prompt);
}

// The AgentOS-owned launcher only translates the generic launch contract. The
// actual ACP implementation remains the upstream pi-acp package.
// @ts-expect-error pi-acp does not publish declarations for its CLI entrypoint.
await import("pi-acp/dist/index.js");
