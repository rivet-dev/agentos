import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Observe permission requests client-side for human-in-the-loop review.
const conn = agent.connect();
conn.on("permissionRequest", (data) => {
  console.log("Permission requested:", data.request);
});
