import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("shared-agent");

// On reconnect, fetch the persisted events and replay the ones after the last
// sequence number we processed. Track `lastSeq` on the client side.
const lastSeq = 42;
const persisted = await agent.getSessionEvents("session-id");
const missedEvents = persisted.filter((e) => e.seq > lastSeq);
for (const event of missedEvents) {
  console.log("Replaying:", event.seq, event.event.method);
}

// Resume live streaming
const conn = agent.connect();
conn.on("sessionEvent", (data) => {
  console.log("Live:", data.event.method);
});
