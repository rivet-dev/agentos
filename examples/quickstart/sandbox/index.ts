// Sandbox extension: mount a Docker sandbox filesystem and run commands.
//
// Requires Docker. Starts a sandbox-agent container, mounts its filesystem
// at /mnt/sandbox, and registers sandbox bindings for running commands.

import { AgentOs } from "@rivet-dev/agentos-core";
import {
	createSandboxFs,
	createSandboxBindings,
} from "@rivet-dev/agentos-sandbox";

const SANDBOX_QUICKSTART_PERMISSIONS = {
	fs: "allow",
	network: "allow",
	childProcess: "allow",
	env: "allow",
	binding: "allow",
} as const;
const skipDocker = process.env.SKIP_DOCKER === "1";
const SANDBOX_MOUNT = "/mnt/sandbox";

if (skipDocker) {
	console.log("Skipping sandbox quickstart because SKIP_DOCKER=1.");
	process.exit(0);
}

const [{ SandboxAgent }, { docker }] = await Promise.all([
	import("sandbox-agent"),
	import("sandbox-agent/docker"),
]);

// Start a Docker-backed sandbox.
const sandbox = await SandboxAgent.start({
	sandbox: docker(),
});

// Mount the sandbox filesystem and register the bindings.
const vm = await AgentOs.create({
	permissions: SANDBOX_QUICKSTART_PERMISSIONS,
	mounts: [
		{
			path: SANDBOX_MOUNT,
			plugin: createSandboxFs({ client: sandbox }),
		},
	],
	bindings: [createSandboxBindings({ client: sandbox })],
});

try {
	// Write and read a file through the mounted sandbox filesystem.
	await vm.writeFile(`${SANDBOX_MOUNT}/hello.txt`, "Hello from agentOS!");
	const content = await vm.readFile(`${SANDBOX_MOUNT}/hello.txt`);
	console.log("Read from sandbox mount:", new TextDecoder().decode(content));

	const runCommandResult = await vm.exec(
		"agentos-sandbox run-command --command echo --args 'hello from Docker sandbox'",
	);
	console.log("Sandbox command:", runCommandResult.stdout.trim());

	const processList = await vm.exec("agentos-sandbox list-processes");
	console.log("Sandbox processes:", processList.stdout.trim());

	const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;
	if (ANTHROPIC_API_KEY) {
		const { sessionId } = await vm.createSession("pi", {
			cwd: SANDBOX_MOUNT,
			env: { ANTHROPIC_API_KEY },
		});
		console.log("Session ID:", sessionId);
		const { text } = await vm.prompt(
			sessionId,
			"Create a C source file named fib.c in the current directory that prints Fibonacci numbers.",
		);
		console.log("Agent:", text);
		if (!(await vm.exists(`${SANDBOX_MOUNT}/fib.c`))) {
			throw new Error(`Expected the agent to create ${SANDBOX_MOUNT}/fib.c`);
		}
		console.log(`Verified ${SANDBOX_MOUNT}/fib.c exists.`);
		await vm.closeSession(sessionId);
	} else {
		console.log("Skipping agent prompt because ANTHROPIC_API_KEY is not set.");
	}
} finally {
	await vm.dispose();
	await sandbox.dispose();
}
