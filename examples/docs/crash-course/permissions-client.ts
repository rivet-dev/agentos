import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Subscribe and reply to permission requests. Permissions are fail-closed, so
// the agent waits until you reply.
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  console.log("Permission requested:", data.request);
  // "once" | "always" | "reject". Reply "always" to auto-approve trusted
  // workloads, or prompt a human for human-in-the-loop.
  await agent.respondPermission(data.sessionId, data.request.permissionId, "once");
});
