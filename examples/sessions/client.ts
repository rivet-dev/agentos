import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

// ── Create a session ──────────────────────────────────────────────
async function createSession() {
	// createSession returns the session ID.
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	console.log(sessionId);
}

// ── createSession options: env ────────────────────────────────────
async function withEnv() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	console.log(sessionId);
}

// ── createSession options: cwd ────────────────────────────────────
async function withCwd() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		cwd: "/home/agentos/project",
	});
	console.log(sessionId);
}

// ── createSession options: additionalInstructions ─────────────────
async function withInstructions() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		additionalInstructions: "Always write tests before implementation.",
	});
	console.log(sessionId);
}

// ── createSession options: skipOsInstructions ─────────────────────
async function withSkipOsInstructions() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		skipOsInstructions: true,
	});
	console.log(sessionId);
}

// ── createSession options: system prompt customization ────────────
async function withSystemPrompt() {
	// docs:start system-prompt
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		// Extra instructions appended to the agent system prompt
		additionalInstructions: "Always write tests before implementation.",
		// Suppress the base OS prompt (binding docs are still injected)
		skipOsInstructions: true,
	});
	// docs:end system-prompt
	console.log(sessionId);
}

// ── Send a prompt ─────────────────────────────────────────────────
async function sendPrompt() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const response = await agent.sendPrompt(
		sessionId,
		"Create a TypeScript function that checks if a number is prime",
	);
	console.log(response.text);
}

// ── Stream responses ──────────────────────────────────────────────
async function streamResponses() {
	const conn = agent.connect();

	// Subscribe to session events before sending the prompt
	conn.on("sessionEvent", (data) => {
		console.log(`[${data.sessionId}]`, data.event.method, data.event.params);
	});

	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	await agent.sendPrompt(sessionId, "Explain how async/await works");
}

// ── Client events ─────────────────────────────────────────────────
function clientEvents() {
	const conn = agent.connect();

	conn.on("sessionEvent", (data) => {
		console.log(data.sessionId, data.event.method, data.event.params);
	});

	conn.on("vmBooted", () => {
		console.log("VM is ready");
	});

	conn.on("vmShutdown", (data) => {
		console.log("VM shutting down:", data.reason);
	});
}

// ── Client events: subscribe before triggering ────────────────────
async function subscribeFirst() {
	const conn = agent.connect();

	// Subscribe first
	conn.on("sessionEvent", (data) => {
		console.log("Session:", data.event.method);
	});

	// Then trigger actions
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	await agent.sendPrompt(sessionId, "Run the test suite");
}

// ── Cancel a prompt ───────────────────────────────────────────────
async function cancelPrompt() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Start a long-running prompt
	const promptPromise = agent.sendPrompt(
		sessionId,
		"Refactor the entire codebase to use TypeScript strict mode",
	);

	// Cancel the in-flight prompt while keeping the session available.
	setTimeout(async () => {
		await agent.cancelPrompt(sessionId);
	}, 10_000);

	const response = await promptPromise;
	console.log(response.text);
}

// ── Close a session ───────────────────────────────────────────────
async function closeSession() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Session events are live-only, so closing releases the session and its stream.
	await agent.closeSession(sessionId);
}

// ── List live sessions ─────────────────────────────────────────────────
async function listLiveSessions() {
	const sessionId = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const sessions = await agent.listSessions();
	for (const session of sessions) {
		console.log(session.sessionId, session.agentType);
	}
}

// ── Multiple sessions ─────────────────────────────────────────────
async function multipleSessions() {
	// Create two sessions in the same VM
	const coder = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const reviewer = await agent.createSession("pi", {
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Coder writes code
	await agent.sendPrompt(coder, "Write a REST API at /home/agentos/api.ts");

	// Reviewer reads and reviews the same file
	await agent.sendPrompt(reviewer, "Review /home/agentos/api.ts for issues");

	// Close each session independently
	await agent.closeSession(coder);
	await agent.closeSession(reviewer);
}

export {
	createSession,
	withEnv,
	withCwd,
	withInstructions,
	withSkipOsInstructions,
	withSystemPrompt,
	sendPrompt,
	streamResponses,
	clientEvents,
	subscribeFirst,
	cancelPrompt,
	closeSession,
	listLiveSessions,
	multipleSessions,
};
