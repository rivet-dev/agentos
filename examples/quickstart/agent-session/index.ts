// Create an agent session and send a prompt using a coding agent.
//
// NOTE: This example requires an API key for the chosen agent and a working
// agent runtime. It may not complete in all environments.

import claude from "@agentos-software/claude-code";
import type { SoftwareInput } from "@rivet-dev/agentos-core";
import { AgentOs } from "@rivet-dev/agentos-core";
import opencode from "@agentos-software/opencode";
import pi from "@agentos-software/pi";

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;

const software: SoftwareInput[] = [claude, opencode, pi];

const vm = await AgentOs.create({
	software,
});

// Change the agent here: "claude", "opencode", or "pi"
const agent = "pi";

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
await vm.closeSession(sessionId);
await vm.dispose();
