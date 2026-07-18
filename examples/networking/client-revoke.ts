import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

// Mint a short-lived preview token so access expires automatically.
const agent = client.vm.getOrCreate("my-agent");
const preview = await agent.createPreviewUrl(3000, 300); // 5 minutes
console.log("Preview path:", preview.path);
console.log("Expires at:", new Date(preview.expiresAt));
