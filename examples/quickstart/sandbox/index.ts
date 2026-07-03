// Give an agent access to a Docker-backed environment for native tools.
//
// Requires Docker and ANTHROPIC_API_KEY. The prompt asks for a normal developer
// task that needs a C compiler; the agent decides how to use the available
// environment and bindings.

import pi from "@agentos-software/pi";
import { AgentOs } from "@rivet-dev/agentos-core";
import { docker } from "@rivet-dev/agentos-sandbox";

const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;
const SANDBOX_QUICKSTART_PERMISSIONS = {
	fs: "allow",
	network: "allow",
	childProcess: "allow",
	env: "allow",
	binding: "allow",
} as const;
const skipDocker = process.env.SKIP_DOCKER === "1";

if (!ANTHROPIC_API_KEY) {
	console.error("Set ANTHROPIC_API_KEY to run this example.");
	process.exit(1);
}

if (skipDocker) {
	console.log("Skipping sandbox quickstart because SKIP_DOCKER=1.");
	process.exit(0);
}

const vm = await AgentOs.create({
	software: [pi],
	permissions: SANDBOX_QUICKSTART_PERMISSIONS,
	sandbox: {
		provider: docker(),
	},
});

try {
	const { sessionId } = await vm.createSession("pi", {
		env: { ANTHROPIC_API_KEY },
	});
	console.log("Session ID:", sessionId);

	vm.onSessionEvent(sessionId, (event) => {
		console.log("Event:", JSON.stringify(event, null, 2));
	});

	const { text } = await vm.prompt(
		sessionId,
		[
			"Create a small C program that prints the first 10 Fibonacci numbers.",
			"Compile it with a C compiler, run the compiled binary, and report the exact output.",
			"If the required compiler or build tools are missing, set up the build environment you need.",
		].join(" "),
	);

	console.log("Agent:", text);
	vm.closeSession(sessionId);
} finally {
	await vm.dispose();
}
