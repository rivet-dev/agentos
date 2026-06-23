import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

const { pid } = await agent.spawn("node", ["/home/agentos/server.js"]);

// List all spawned processes
const processes = await agent.listProcesses();
console.log(processes);

// Get info about a specific process
const info = await agent.getProcess(pid);
console.log(info.running, info.exitCode);

// Graceful stop (SIGTERM)
await agent.stopProcess(pid);

// Force kill (SIGKILL)
await agent.killProcess(pid);
