import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Stream events as they arrive
const conn = agent.connect();
conn.on("sessionEvent", (data) => {
  console.log(data.event.method, data.event);
});

// Create a session
const sessionId = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// Send a prompt and wait for the response
const response = await agent.sendPrompt(
  sessionId,
  "List all files in the home directory",
);
console.log(response.text);
