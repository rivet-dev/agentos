import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;
if (!ANTHROPIC_API_KEY) {
	console.error("Set ANTHROPIC_API_KEY to run this example.");
	process.exit(1);
}

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const vm = client.vm.getOrCreate("native-tools-demo");

const conn = vm.connect();
conn.on("sessionEvent", (data) => {
	console.log(data.event);
});

const sessionId = await vm.createSession("pi", {
	env: { ANTHROPIC_API_KEY },
});

const response = await vm.sendPrompt(
	sessionId,
	[
		"Create a small C program that prints the first 10 Fibonacci numbers.",
		"Compile it with a C compiler, run the compiled binary, and report the exact output.",
		"If the required compiler or build tools are missing, set up the build environment you need.",
	].join(" "),
);

console.log(response.text);
await vm.closeSession(sessionId);
