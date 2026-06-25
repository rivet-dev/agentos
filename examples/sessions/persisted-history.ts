import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

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
