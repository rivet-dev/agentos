import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = client.codeReviewer.getOrCreate("main");

// Run the chained review + fix workflow against a file in the VM.
await handle.send("codeReview", { filePath: "/home/agentos/src/auth.ts" });
