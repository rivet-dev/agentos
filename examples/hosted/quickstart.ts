import { createAgentOsClient } from "@rivet-dev/agentos";

// Connect to Rivet Cloud. Set RIVET_NAMESPACE and RIVET_TOKEN from your
// project's dashboard before running this.
const client = createAgentOsClient({ endpoint: "https://api.rivet.dev" });

// Get the VM keyed "my-vm", creating it on first use. Same key, same VM.
const vm = client.agentOS.getOrCreate(["my-vm"]);

// Write a file. The VM's filesystem persists while the VM sleeps.
await vm.v1.filesystem.writeFile({
	path: "/home/agentos/hello.txt",
	content: "Hello from agentOS!",
});

// Run a shell command and capture its output.
const result = await vm.v1.process.run({
	command: "cat",
	args: ["/home/agentos/hello.txt"],
	options: { captureStdio: true },
});
console.log(result.stdout);

// Start a web server inside the VM.
await vm.v1.filesystem.writeFile({
	path: "/home/agentos/server.js",
	content: `require("http")
	.createServer((req, res) => res.end("Hello from the VM!"))
	.listen(3000);`,
});
await vm.v1.process.spawn({
	command: "node",
	args: ["/home/agentos/server.js"],
});

// Expose the server through a preview URL that expires in one hour.
const preview = await vm.v1.network.preview.create({
	port: 3000,
	ttlMs: 60 * 60 * 1_000,
});
console.log(preview.path);
