import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./review-server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

// Wait for the agent to complete the review
const result = await client.reviewer.getOrCreate("main").send(
  "review",
  { file: "/home/user/src/auth.ts" },
  { wait: true, timeout: 120_000 },
);

if (result.status === "completed") {
  console.log("Review:", result.response?.summary);
}
