import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

// Client A: creates the session and sends prompts
const clientA = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agentA = clientA.vm.getOrCreate("shared-agent");
const connA = agentA.connect();
connA.on("sessionEvent", (data) =>
  console.log("[A]", data.event.method),
);

const session = await agentA.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agentA.sendPrompt(session.sessionId, "Build a REST API");

// Client B: observes the same session (separate process)
const clientB = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const connB = clientB.vm.getOrCreate("shared-agent").connect();
connB.on("sessionEvent", (data) =>
  console.log("[B]", data.event.method),
);
// Client B sees the same events as Client A
