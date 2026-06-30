import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// `data.request.description` and `data.request.params` carry the raw ACP
// permission details (the requested tool, paths, etc.). Inspect them to decide
// which requests to approve automatically and which to send to a human.
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  const description = data.request.description ?? "";
  if (description.toLowerCase().includes("read")) {
    // Auto-approve read requests.
    await agent.respondPermission(data.sessionId, data.request.permissionId, "once");
    return;
  }
  // Everything else goes to a human.
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
