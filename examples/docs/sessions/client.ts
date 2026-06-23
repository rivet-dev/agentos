import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// ── Create a session ──────────────────────────────────────────────
async function createSession() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  console.log(session.sessionId);
  console.log(session.capabilities);
  console.log(session.agentInfo);
}

// ── createSession options: env ────────────────────────────────────
async function withEnv() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  console.log(session.sessionId);
}

// ── createSession options: cwd ────────────────────────────────────
async function withCwd() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    cwd: "/home/agentos/project",
  });
  console.log(session.sessionId);
}

// ── createSession options: local MCP server ───────────────────────
async function withLocalMcp() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    mcpServers: [
      {
        type: "local",
        command: "npx",
        args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/agentos"],
        env: {},
      },
    ],
  });
  console.log(session.sessionId);
}

// ── createSession options: remote MCP server ──────────────────────
async function withRemoteMcp() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    mcpServers: [
      {
        type: "remote",
        url: "https://mcp.example.com/sse",
        headers: {
          Authorization: "Bearer my-token",
        },
      },
    ],
  });
  console.log(session.sessionId);
}

// ── createSession options: additionalInstructions ─────────────────
async function withInstructions() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    additionalInstructions: "Always write tests before implementation.",
  });
  console.log(session.sessionId);
}

// ── createSession options: skipOsInstructions ─────────────────────
async function withSkipOsInstructions() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
    skipOsInstructions: true,
  });
  console.log(session.sessionId);
}

// ── Send a prompt ─────────────────────────────────────────────────
async function sendPrompt() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  const response = await agent.sendPrompt(
    session.sessionId,
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

  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agent.sendPrompt(session.sessionId, "Explain how async/await works");
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
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agent.sendPrompt(session.sessionId, "Run the test suite");
}

// ── Cancel a prompt ───────────────────────────────────────────────
async function cancelPrompt() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Start a long-running prompt
  const promptPromise = agent.sendPrompt(
    session.sessionId,
    "Refactor the entire codebase to use TypeScript strict mode",
  );

  // Cancel after 10 seconds
  setTimeout(async () => {
    await agent.cancelPrompt(session.sessionId);
  }, 10_000);

  const response = await promptPromise;
  console.log(response.text);
}

// ── Close and destroy sessions ────────────────────────────────────
async function closeAndDestroy() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Close without destroying persisted data
  await agent.closeSession(session.sessionId);

  // Destroy session and all persisted events
  await agent.destroySession(session.sessionId);
}

// ── Runtime configuration ─────────────────────────────────────────
async function runtimeConfig() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });

  // Change model
  await agent.setModel(session.sessionId, "claude-sonnet-4-6");

  // Change mode (e.g. "plan", "auto")
  await agent.setMode(session.sessionId, "plan");

  // Change thought level
  await agent.setThoughtLevel(session.sessionId, "high");

  // Query available options
  const modes = await agent.getModes(session.sessionId);
  console.log(modes);

  const options = await agent.getConfigOptions(session.sessionId);
  console.log(options);
}

// ── Replay events ─────────────────────────────────────────────────
async function replayEvents() {
  const session = await agent.createSession("pi", {
    env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  });
  await agent.sendPrompt(session.sessionId, "Hello");

  // Replay persisted events
  const events = await agent.getSessionEvents(session.sessionId);
  console.log(events);

  // Get in-memory events with sequence numbers (for reconnection)
  const sequenced = await agent.getSequencedEvents(session.sessionId, {
    since: 0,
  });
  console.log(sequenced);
}

// ── Persisted session history ─────────────────────────────────────
async function persistedHistory() {
  // List all persisted sessions
  const sessions = await agent.listPersistedSessions();
  for (const s of sessions) {
    console.log(s.sessionId, s.agentType, s.createdAt);
  }

  // Get full event history for a session
  const events = await agent.getSessionEvents(sessions[0].sessionId);
  for (const e of events) {
    console.log(e.seq, e.event.method, e.createdAt);
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
  await agent.sendPrompt(coder.sessionId, "Write a REST API at /home/agentos/api.ts");

  // Reviewer reads and reviews the same file
  await agent.sendPrompt(reviewer.sessionId, "Review /home/agentos/api.ts for issues");

  // Close each session independently
  await agent.closeSession(coder.sessionId);
  await agent.closeSession(reviewer.sessionId);
}

export {
  createSession,
  withEnv,
  withCwd,
  withLocalMcp,
  withRemoteMcp,
  withInstructions,
  withSkipOsInstructions,
  sendPrompt,
  streamResponses,
  clientEvents,
  subscribeFirst,
  cancelPrompt,
  closeAndDestroy,
  runtimeConfig,
  replayEvents,
  persistedHistory,
  multipleSessions,
};
