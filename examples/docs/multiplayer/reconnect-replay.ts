import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("shared-agent");

// On reconnect, replay events from the last known sequence number
const lastSeq = 42; // Track this on the client side
const missedEvents = await agent.getSequencedEvents("session-id", {
  since: lastSeq,
});
for (const event of missedEvents) {
  console.log("Replaying:", event.sequenceNumber, event.notification.method);
}

// Resume live streaming
const conn = agent.connect();
conn.on("sessionEvent", (data) => {
  console.log("Live:", data.event.method);
});
