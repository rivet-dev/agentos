import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.bugFixer.getOrCreate("main");

// Trigger the durable workflow by sending to its queue. The workflow runs each
// step in order against the VM, surviving restarts: the output of one step (the
// cloned repo, the agent's edits) feeds into the next.
await handle.send("fixBug", {
  repo: "https://github.com/example/repo.git",
  issue: "Fix the login redirect bug",
});

const state = await handle.getState();
console.log("Last issue:", state.lastIssue, "exit code:", state.lastExitCode);
