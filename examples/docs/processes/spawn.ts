import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");
const conn = agent.connect();

// Subscribe to process output
conn.on("processOutput", (data) => {
  const text = new TextDecoder().decode(data.data);
  console.log(`[pid ${data.pid}] ${data.stream}: ${text}`);
});

conn.on("processExit", (data) => {
  console.log(`[pid ${data.pid}] exited with code ${data.exitCode}`);
});

// Spawn a dev server
const { pid } = await agent.spawn("node", ["/home/user/server.js"]);
console.log("Started process:", pid);
