import { randomUUID } from "node:crypto";
import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });

// Creating one actor starts one durable workflow with immutable input.
const handle = await client.bugFixer.create(randomUUID(), {
	input: {
		repo: "https://github.com/example/repo.git",
		issue: "Fix the login redirect bug",
	},
});

let state = await handle.getState();
while (state.status !== "complete") {
	await new Promise((resolve) => setTimeout(resolve, 1_000));
	state = await handle.getState();
}
console.log("Exit code:", state.exitCode);
