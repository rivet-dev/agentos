// Host-side mock LLM server for the Pi prompt-turn e2e gate. Starts `@copilotkit/llmock` on a random
// port, replies to any request with a fixed sentinel, prints `LLMOCK_URL=<url>` to stdout, then stays
// alive until killed. Run from the repo root so `@copilotkit/llmock` resolves. This runs on the HOST
// (the mock LLM the VM calls out to), exactly like the TypeScript Pi tests; it is test infrastructure,
// not guest code.
import { LLMock } from "@copilotkit/llmock";

const sentinel = process.env.LLMOCK_SENTINEL ?? "PONG_FROM_LLMOCK";
const mock = new LLMock({ port: 0, logLevel: "silent" });
mock.addFixtures([{ match: { predicate: () => true }, response: { content: sentinel } }]);

const url = await mock.start();
process.stdout.write(`LLMOCK_URL=${url}\n`);

// Stay alive until the parent test kills us.
process.stdin.resume();
