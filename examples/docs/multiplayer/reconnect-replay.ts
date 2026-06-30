import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("shared-agent");

// On reconnect, read back the full persisted session event history. ACP session
// events are live-only — there is no sequenced cursor replay — so getSessionEvents
// returns the complete ordered array of raw JSON-RPC notifications.
const events = await agent.getSessionEvents("session-id");
for (const event of events) {
  console.log("Replaying:", event.method, event.params);
}

// Resume live streaming
const conn = agent.connect();
conn.on("sessionEvent", (data) => {
  console.log("Live:", data.event.method);
});
