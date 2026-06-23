import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");
const conn = agent.connect();

// Subscribe to shell output
conn.on("shellData", (data) => {
  const text = new TextDecoder().decode(data.data);
  process.stdout.write(text);
});

// Open a shell
const { shellId } = await agent.openShell();

// Write commands to the shell
await agent.writeShell(shellId, "ls -la /home/agentos\n");

// Resize the terminal
await agent.resizeShell(shellId, 120, 40);

// Close the shell when done
await agent.closeShell(shellId);
