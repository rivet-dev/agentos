import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Permission requests forwarded by the server reach the client here. The
// payload is inferred from the actor's event schema, so no cast is needed.
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  const approved = confirm(`Allow: ${JSON.stringify(data.request)}?`);
  await agent.respondPermission(
    data.sessionId,
    data.request.permissionId,
    approved ? "once" : "reject",
  );
});

const session = await agent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});
await agent.sendPrompt(session.sessionId, "Read config.json and update it");
