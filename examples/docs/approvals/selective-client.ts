import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  // Inspect the request and decide per-request. `request.description` /
  // `request.params` carry the raw ACP details (the requested tool, paths, etc.).
  const description = data.request.description?.toLowerCase() ?? "";
  if (description.includes("read")) {
    // Auto-approve reads.
    await agent.respondPermission(data.sessionId, data.request.permissionId, "always");
    return;
  }
  // Forward everything else to a human.
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
