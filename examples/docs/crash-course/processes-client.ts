import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// One-shot execution
const result = await agent.exec("echo hello && ls /home/user");
console.log("stdout:", result.stdout);
console.log("exit code:", result.exitCode);

// Spawn a long-running process
const conn = agent.connect();
conn.on("processOutput", (data) => {
  console.log(`[pid ${data.pid}]`, new TextDecoder().decode(data.data));
});

const { pid } = await agent.spawn("node", ["server.js"]);
console.log("Process ID:", pid);
