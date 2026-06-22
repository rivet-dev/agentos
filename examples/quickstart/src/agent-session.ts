// Create an agent session and send a prompt using a coding agent.
//
// NOTE: This example requires an API key for the chosen agent and a working
// agent runtime. It may not complete in all environments.

import claude from "@rivet-dev/agentos-claude";
import common from "@agent-os-pkgs/common";
import type { SoftwareInput } from "@rivet-dev/agentos-core";
import { AgentOs } from "@rivet-dev/agentos-core";
import opencode from "@rivet-dev/agentos-opencode";
import pi from "@rivet-dev/agentos-pi";

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;

const software: SoftwareInput[] = [common, claude, opencode, pi];

const vm = await AgentOs.create({
	software,
});

// Change the agent here: "claude", "opencode", or "pi"
const agent = "claude";

const env: Record<string, string> = {};
if (ANTHROPIC_API_KEY) env.ANTHROPIC_API_KEY = ANTHROPIC_API_KEY;

const { sessionId } = await vm.createSession(agent, { env });
console.log("Session ID:", sessionId);

// Listen for session events (streamed text, tool use, etc.)
vm.onSessionEvent(sessionId, (event) => {
	console.log("Event:", JSON.stringify(event, null, 2));
});

// Send a prompt and wait for the response
const { text } = await vm.prompt(
	sessionId,
	"What is 2 + 2? Reply with just the number.",
);
console.log("Response:", text);

// Close the session
vm.closeSession(sessionId);
await vm.dispose();
