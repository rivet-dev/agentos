import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Or handle permissions client-side for human-in-the-loop
const conn = agent.connect();
conn.on("permissionRequest", async (data) => {
  console.log("Permission requested:", data.request);
  // "once" | "always" | "reject"
  await agent.respondPermission(data.sessionId, data.request.permissionId, "once");
});
