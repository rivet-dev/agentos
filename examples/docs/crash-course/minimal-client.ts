import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Subscribe to streaming events
const conn = agent.connect();
conn.on("sessionEvent", (data) => {
  console.log(data.event);
});

// Create a session and send a prompt
const session = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
const response = await agent.sendPrompt(
  session.sessionId,
  "Write a hello world script to /home/user/hello.js",
);
console.log(response.text);

// Read the file the agent created
const content = await agent.readFile("/home/user/hello.js");
console.log(new TextDecoder().decode(content));
