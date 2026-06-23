import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const vm = client.vm.getOrCreate("my-agent");

// Write code via the filesystem. The /home/user/sandbox mount maps to the sandbox root.
await vm.writeFile("/home/user/sandbox/app/index.ts", 'console.log("hello")');

// Run it inside the sandbox. Commands execute through the VM's process table,
// reading the file from the mounted directory.
const result = await vm.exec("node /home/user/sandbox/app/index.ts");
console.log(result.stdout); // "hello\n"

// Call a mounted binding from the client. The sandbox bindings are exposed inside
// the VM as a CLI command, so you invoke it through the same exec/spawn surface.
const install = await vm.exec("agentos-sandbox run-command --command \"npm install\" --cwd /home/user/sandbox/app");
console.log(install.exitCode, install.stdout);

// Spawn a long-running process via the bindings and stream its output.
const { pid } = await vm.spawn("agentos-sandbox", [
	"create-process",
	"--command",
	"npm",
	"--args",
	"run",
	"--args",
	"dev",
]);
const conn = vm.connect();
conn.on("processOutput", (payload) => {
	if (payload.pid === pid) {
		console.log(payload.stream, new TextDecoder().decode(payload.data));
	}
});
