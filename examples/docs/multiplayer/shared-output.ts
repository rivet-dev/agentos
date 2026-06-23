import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const conn = client.vm.getOrCreate("shared-agent").connect();

// All connected clients see process output
conn.on("processOutput", (data) => {
  const text = new TextDecoder().decode(data.data);
  console.log(`[pid ${data.pid}] ${data.stream}: ${text}`);
});

// All connected clients see shell data
conn.on("shellData", (data) => {
  const text = new TextDecoder().decode(data.data);
  process.stdout.write(text);
});
