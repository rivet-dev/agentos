import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

// Pass credentials when connecting. They are forwarded as the connection
// params for your server-side validation hooks to check. `params` is typed as
// unknown, so the shape is not checked against the actor's ConnParams here.
const agent = client.vm.getOrCreate("my-agent", {
  params: { authToken: "my-jwt-token" },
});

// Actions on the handle run against the authenticated connection.
// `createSession` resolves to the session ID string.
const sessionId = await agent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agent.sendPrompt(sessionId, "List the files in the working directory.");
