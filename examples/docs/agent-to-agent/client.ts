import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const writerAgent = client.writer.getOrCreate("my-project");

const session = await writerAgent.createSession("claude", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// The writer calls the `review` binding, which bridges to the reviewer VM.
await writerAgent.sendPrompt(
  session.sessionId,
  "Write a REST API at /home/agentos/api.ts, then run `agentos-review submit --path /home/agentos/api.ts` to have it reviewed.",
);
