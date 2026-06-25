import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.vm.getOrCreate("my-agent");

// Subscribe to streaming events. The payload is inferred from the event schema.
const conn = handle.connect();
conn.on("sessionEvent", (data) => {
  console.log(data.event);
});

// Create a session and send a prompt. createSession returns the session ID.
const sessionId = await handle.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await handle.sendPrompt(
  sessionId,
  "Write a hello world script to /workspace/hello.js",
);

// Read the file the agent created
const content = await handle.readFile("/workspace/hello.js");
console.log(new TextDecoder().decode(content));
