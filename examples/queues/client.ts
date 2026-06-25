import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.taskRunner.getOrCreate("main");

// Queue up work. Tasks are processed one at a time.
await handle.send("tasks", { prompt: "Review PR #123" });
await handle.send("tasks", { prompt: "Fix the flaky test in auth.test.ts" });
await handle.send("tasks", { prompt: "Update the README" });
