import { vm } from "./client.js";

const spawned = await vm.process.spawn({
	command: "cat",
	args: [],
	options: { env: {} },
});

// Write to stdin
await vm.process.stdin.write({ process: spawned, data: "hello from stdin\n" });

// Close stdin when done
await vm.process.stdin.close({ process: spawned });

// Wait for the process to exit
const exit = await vm.process.wait({ process: spawned });
console.log("exit code:", exit.status.exitCode);
