import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

// Driver client
const driver = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const driverAgent = driver.vm.getOrCreate("shared-agent");

const sessionId = await driverAgent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// Observer client (different user, same actor)
const observer = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

const observerConn = observer.vm.getOrCreate("shared-agent").connect();
observerConn.on("sessionEvent", (data) => {
  console.log("[observer]", data.event.method, data.event.params);
});

// Driver sends a prompt. Observer sees the streaming response.
await driverAgent.sendPrompt(sessionId, "Refactor the auth module");
