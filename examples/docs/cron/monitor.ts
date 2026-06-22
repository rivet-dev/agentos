import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.vm.getOrCreate("my-agent");

const conn = handle.connect();
conn.on("cronEvent", (data) => {
  // data is inferred: { event: CronEvent }
  console.log("Cron event:", data.event);
});

await handle.scheduleCron({
  schedule: "*/1 * * * *",
  action: { type: "exec", command: "echo", args: ["heartbeat"] },
});
