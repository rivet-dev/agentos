import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./agent-to-agent-server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const coderAgent = client.coder.getOrCreate("feature-auth");
const session = await coderAgent.createSession("pi", {
  env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

// The coder implements the feature, then calls the `review` binding itself so the
// reviewer agent reviews the code. This is true agent-to-agent: the coder drives it.
await coderAgent.sendPrompt(
  session.sessionId,
  "Implement the login feature in /home/user/src/auth.ts, then run `agentos-review submit --path /home/user/src/auth.ts` to have it reviewed.",
);
