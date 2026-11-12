import { AgentOs } from "@rivet-dev/agentos";

// Boots a VM inside this process. It runs until dispose() or process exit.
const vm = await AgentOs.create();

try {
	// Bash
	const result = await vm.process.exec("echo hello && ls /home/agentos", {
		output: { capture: "all" },
	});
	console.log(result.stdout);

	// Filesystem
	await vm.filesystem.writeFile("/home/agentos/hello.txt", "Hello, world!");
	const file = await vm.filesystem.readFile("/home/agentos/hello.txt");
	console.log(new TextDecoder().decode(file));

	// JavaScript
	await vm.javascript.execute("console.log(1 + 2)");

	// Python
	const answer = await vm.python.evaluate<number>("21 * 2");
	console.log(answer.outcome === "succeeded" ? answer.value : answer.error);
} finally {
	await vm.dispose();
}
