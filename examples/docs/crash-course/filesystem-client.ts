import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Write a file
await agent.writeFile("/home/user/config.json", JSON.stringify({ key: "value" }));

// Read a file
const content = await agent.readFile("/home/user/config.json");
console.log(new TextDecoder().decode(content));

// List directory contents recursively
const files = await agent.readdirRecursive("/home/user", { maxDepth: 2 });
console.log(files);
