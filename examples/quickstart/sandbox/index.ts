// Sandbox extension: mount a Docker sandbox filesystem and run commands.
//
// Requires Docker. Starts a sandbox-agent container, mounts its filesystem
// at /sandbox, and registers sandbox bindings for running commands.

import { AgentOs } from "@rivet-dev/agentos-core";
import {
	createSandboxBindings,
	createSandboxFs,
} from "@rivet-dev/agentos-sandbox";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

const SANDBOX_QUICKSTART_PERMISSIONS = {
	fs: "allow",
	network: "allow",
	childProcess: "allow",
	env: "allow",
	binding: "allow",
} as const;
const skipDocker = process.env.SKIP_DOCKER === "1";

if (skipDocker) {
	console.log("Skipping sandbox quickstart because SKIP_DOCKER=1.");
	process.exit(0);
}

const sandbox = await SandboxAgent.start({
	sandbox: docker(),
});

const vm = await AgentOs.create({
	permissions: SANDBOX_QUICKSTART_PERMISSIONS,
	mounts: [
		{
			path: "/sandbox",
			plugin: createSandboxFs({ client: sandbox }),
		},
	],
	bindings: [createSandboxBindings({ client: sandbox })],
});

try {
	// Write and read a file through the mounted sandbox filesystem.
	await vm.writeFile("/sandbox/hello.txt", "Hello from agentOS!");
	const content = await vm.readFile("/sandbox/hello.txt");
	console.log("Read from sandbox mount:", new TextDecoder().decode(content));

	const runCommandResult = await vm.exec(
		"agentos-sandbox run-command --command echo --args 'hello from Docker sandbox'",
	);
	console.log("Sandbox command:", JSON.stringify(runCommandResult));

	const processList = await vm.exec("agentos-sandbox list-processes");
	console.log("Sandbox processes:", JSON.stringify(processList));
} finally {
	await vm.dispose();
	await sandbox.dispose();
}
