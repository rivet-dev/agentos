import { AgentOs } from "@rivet-dev/agentos-core";
import { docker } from "@rivet-dev/agentos-sandbox";

// Sandbox Agent mounts and host bindings are trusted embedded Core features.
const vm = await AgentOs.create({
	sandbox: {
		provider: docker(),
		mountPath: "/home/agentos/sandbox",
	},
});

// Write code via the filesystem. The /home/agentos/sandbox mount maps to the sandbox root.
await vm.filesystem.writeFile(
	"/home/agentos/sandbox/app/index.ts",
	'console.log("hello")',
);

// Run it inside the sandbox through the generated binding command.
// The VM path above maps to /app/index.ts at the sandbox root.
const result = await vm.process.exec(
	"agentos-sandbox run-command --command node --args /app/index.ts",
);
console.log(result.stdout); // "hello\n"

const install = await vm.process.exec(
	"agentos-sandbox run-command --command npm --args install --args --prefix --args /app",
);
console.log(install.exitCode, install.stdout);

// Spawn a long-running process and stream its output. Connect to the VM,
// then subscribe to `processOutput` events for the spawned pid.
const { pid } = await vm.process.spawn("npm", [
	"run",
	"dev",
	"--prefix",
	"/home/agentos/sandbox/app",
]);
vm.onProcessOutput(pid, (payload) => {
	console.log(payload.stream, new TextDecoder().decode(payload.data));
});
