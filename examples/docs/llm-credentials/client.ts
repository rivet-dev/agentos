import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});

// Pass LLM provider keys via the `env` option on createSession. The VM does
// not inherit from the host process.env, so keys must be passed explicitly.
const session = await client.vm.getOrCreate("my-agent").createSession("pi", {
	env: { ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY! },
});

console.log(session.sessionId);
