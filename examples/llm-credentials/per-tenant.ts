import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});

// Stand-in for your own per-tenant credential store.
declare function lookupTenantApiKey(tenantId: string): Promise<string>;

// Give each tenant an isolated VM keyed by their tenant id, then inject that
// tenant's API key from your database at session creation. Keys stay on the
// server and never reach the client.
async function startTenantSession(tenantId: string) {
	const anthropicApiKey = await lookupTenantApiKey(tenantId);

	return client.vm.getOrCreate(tenantId).createSession("pi", {
		env: { ANTHROPIC_API_KEY: anthropicApiKey },
	});
}

const sessionId = await startTenantSession("tenant-123");
console.log(sessionId);
