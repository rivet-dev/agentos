import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./ingest-server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

// Ingest from a webhook or external system
await client.issueWorker.getOrCreate("main").ingestIssue(
  "Login redirect broken",
  "Users are redirected to /undefined after login on mobile",
);
