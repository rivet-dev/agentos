import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const agent = client.vm.getOrCreate("my-agent");

// Fetch from a service running inside the VM
const response = await agent.vmFetch(3000, "/api/health");
console.log("Status:", response.status);

// Create a preview URL (port forwarding to a public URL), valid for 1 hour
const preview = await agent.createSignedPreviewUrl(3000, 3600);
console.log("Public URL:", preview.url);
console.log("Expires at:", new Date(preview.expiresAt));
