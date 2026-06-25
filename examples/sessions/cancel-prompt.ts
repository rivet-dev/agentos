import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

const sessionId = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// Start a long-running prompt
const promptPromise = agent.sendPrompt(
  sessionId,
  "Refactor the entire codebase to use TypeScript strict mode",
);

// Closing the session cancels the in-flight prompt and releases its resources.
setTimeout(async () => {
  await agent.closeSession(sessionId);
}, 10_000);

const response = await promptPromise;
console.log(response.text);
