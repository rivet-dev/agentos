import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

const { pid } = await agent.spawn("cat", []);

// Write to stdin
await agent.writeProcessStdin(pid, "hello from stdin\n");

// Close stdin when done
await agent.closeProcessStdin(pid);

// Wait for the process to exit
const exitCode = await agent.waitProcess(pid);
console.log("exit code:", exitCode);
