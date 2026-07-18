import { randomUUID } from "node:crypto";
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const handle = await client.codeReviewer.create(randomUUID(), {
	input: { filePath: "/home/agentos/src/auth.ts" },
});

let state = await handle.getState();
while (state.status !== "complete") {
	await new Promise((resolve) => setTimeout(resolve, 1_000));
	state = await handle.getState();
}
