import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Model, mode, and thought level are configured per session at creation time
// through createSession options and the agent's own configuration. There is no
// runtime mutation API on the VM handle.
const sessionId = await agent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
  additionalInstructions: "Prefer a plan-first workflow and explain your reasoning.",
});
await agent.sendPrompt(sessionId, "Outline a plan before writing any code.");
