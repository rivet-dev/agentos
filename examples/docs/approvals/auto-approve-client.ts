import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Auto-approve every request as it arrives. `"always"` also approves future
// requests of the same type, so a multi-step agent run is not interrupted.
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  await agent.respondPermission(data.sessionId, data.request.permissionId, "always");
});

const session = await agent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agent.sendPrompt(session.sessionId, "Write files as needed");
