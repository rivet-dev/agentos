import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

const { pid } = await agent.spawn("node", ["/home/agentos/server.js"]);

// List all processes tracked by the VM
const processes = await agent.listProcesses();
for (const p of processes) {
  console.log(p.pid, p.command, p.args.join(" "), p.status);
}

// Inspect a specific process by pid
const info = await agent.getProcess(pid);
console.log(info.status, info.exitCode);

// Graceful stop (SIGTERM)
await agent.stopProcess(pid);

// Force kill (SIGKILL)
await agent.killProcess(pid);
