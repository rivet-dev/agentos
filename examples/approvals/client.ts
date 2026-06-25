import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Listen for permission requests over a live connection. The payload is
// inferred from the actor's event schema, so no cast is needed.
const conn = agent.connect();
conn.on("permissionRequest", (data) => {
  console.log("Permission requested:", data.request);
});

const sessionId = await agent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agent.sendPrompt(sessionId, "Create a new file at /home/agentos/output.txt");
