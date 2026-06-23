import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const vm = client.vm.getOrCreate("my-agent");

// List sessions persisted before sleep (works without a running VM)
const sessions = await vm.listPersistedSessions();
console.log("Previous sessions:", sessions.length);

// Replay the most recent session's transcript from durable storage
const last = sessions[0];
if (last) {
  const events = await vm.getSessionEvents(last.sessionId);
  for (const e of events) {
    console.log(e.seq, e.event.method);
  }
}
