import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Auto-approve every request as it arrives. Reply "always" so future requests
// of the same type are approved without another prompt.
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  console.log("auto-approving", data.sessionId, data.request.permissionId);
  await agent.respondPermission(data.sessionId, data.request.permissionId, "always");
});

const session = await agent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agent.sendPrompt(session.sessionId, "Write files as needed");
