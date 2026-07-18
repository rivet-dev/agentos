import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const agent = client.vm.getOrCreate("my-agent");

// ── Open a session ──────────────────────────────────────────────
async function openSession() {
	// The caller owns the durable session ID; openSession resolves with no value.
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
}

// ── openSession options: env ────────────────────────────────────
async function withEnv() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
}

// ── openSession options: cwd ────────────────────────────────────
async function withCwd() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		cwd: "/home/agentos/project",
	});
}

// ── openSession options: additionalInstructions ─────────────────
async function withInstructions() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		additionalInstructions: "Always write tests before implementation.",
	});
}

// ── openSession options: skipOsInstructions ─────────────────────
async function withSkipOsInstructions() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		skipOsInstructions: true,
	});
}

// ── openSession options: system prompt customization ────────────
async function withSystemPrompt() {
	// docs:start system-prompt
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
		// Extra instructions appended to the agent system prompt
		additionalInstructions: "Always write tests before implementation.",
		// Suppress the base OS prompt (binding docs are still injected)
		skipOsInstructions: true,
	});
	// docs:end system-prompt
}

// ── Prompt a session ─────────────────────────────────────────────────
async function promptSession() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const response = await agent.prompt({
		content: [
			{
				type: "text",
				text: "Create a TypeScript function that checks if a number is prime",
			},
		],
	});
	console.log(response.message?.content ?? []);
}

// ── Stream responses ──────────────────────────────────────────────
async function streamResponses() {
	const conn = agent.connect();

	// Subscribe to session events before sending the prompt
	conn.on("sessionEvent", (event) => {
		console.log(`[${event.sessionId}]`, event.durability, event);
	});

	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	await agent.prompt({
		content: [{ type: "text", text: "Explain how async/await works" }],
	});
}

// ── Client events ─────────────────────────────────────────────────
function clientEvents() {
	const conn = agent.connect();

	conn.on("sessionEvent", (event) => {
		console.log(event.sessionId, event.durability, event);
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
	conn.on("sessionEvent", (event) => {
		console.log("Session:", event);
	});

	// Then trigger actions
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	await agent.prompt({
		content: [{ type: "text", text: "Run the test suite" }],
	});
}

// ── Cancel a prompt ───────────────────────────────────────────────
async function cancelPrompt() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Start a long-running prompt
	const promptPromise = agent.prompt({
		content: [
			{
				type: "text",
				text: "Refactor the entire codebase to use TypeScript strict mode",
			},
		],
	});

	// Cancel the in-flight prompt while keeping the session available.
	setTimeout(async () => {
		await agent.cancelPrompt();
	}, 10_000);

	const response = await promptPromise;
	console.log(response.message?.content ?? []);
}

// ── Unload a session runtime ───────────────────────────────────────────────
async function unloadSession() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Release the adapter without deleting the durable session or its history.
	await agent.unloadSession();
}

// ── List durable sessions ─────────────────────────────────────────────────
async function listDurableSessions() {
	await agent.openSession({
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const page = await agent.listSessions();
	for (const session of page.sessions) {
		console.log(session.sessionId, session.agent);
	}
}

// ── Multiple sessions ─────────────────────────────────────────────
async function multipleSessions() {
	// Create two sessions in the same VM
	const coderSessionId = "coder";
	await agent.openSession({
		sessionId: coderSessionId,
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});
	const reviewerSessionId = "reviewer";
	await agent.openSession({
		sessionId: reviewerSessionId,
		agent: "pi",
		env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
	});

	// Coder writes code
	await agent.prompt({
		sessionId: coderSessionId,
		content: [
			{ type: "text", text: "Write a REST API at /home/agentos/api.ts" },
		],
	});

	// Reviewer reads and reviews the same file
	await agent.prompt({
		sessionId: reviewerSessionId,
		content: [{ type: "text", text: "Review /home/agentos/api.ts for issues" }],
	});

	// Unload each adapter independently while preserving both histories.
	await agent.unloadSession({ sessionId: coderSessionId });
	await agent.unloadSession({ sessionId: reviewerSessionId });
}

export {
	openSession,
	withEnv,
	withCwd,
	withInstructions,
	withSkipOsInstructions,
	withSystemPrompt,
	promptSession,
	streamResponses,
	clientEvents,
	subscribeFirst,
	cancelPrompt,
	unloadSession,
	listDurableSessions,
	multipleSessions,
};
