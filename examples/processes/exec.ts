import { vm } from "./client.js";

const result = await vm.process.run({
	command: "sh",
	args: ["-c", "echo hello && ls /home/agentos"],
	options: { env: {}, captureStdio: true },
});
console.log("stdout:", result.stdout);
console.log("stderr:", result.stderr);
console.log("exit code:", result.status.exitCode);
