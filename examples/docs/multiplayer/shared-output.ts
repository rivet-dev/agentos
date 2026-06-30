import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const conn = client.vm.getOrCreate("shared-agent").connect();

// Session events are the only events broadcast to every subscriber on the actor.
// All connected clients receive them.
conn.on("sessionEvent", (data) => {
  console.log(data.event.method, data.event.params);
});
