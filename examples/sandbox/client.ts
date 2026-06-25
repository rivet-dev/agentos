import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({ endpoint: "http://localhost:6420" });
const vm = client.vm.getOrCreate("my-agent");

// Write code via the filesystem. The /home/agentos/sandbox mount maps to the sandbox root.
await vm.writeFile("/home/agentos/sandbox/app/index.ts", 'console.log("hello")');

// Run it inside the sandbox. Commands execute through the VM's process table,
// reading the file from the mounted directory.
const result = await vm.exec("node /home/agentos/sandbox/app/index.ts");
console.log(result.stdout); // "hello\n"

// Run a command against the mounted app directory. Because the sandbox
// filesystem is mounted into the VM, commands operate on the same files.
const install = await vm.exec("npm install --prefix /home/agentos/sandbox/app");
console.log(install.exitCode, install.stdout);

// Spawn a long-running process and stream its output. Connect to the VM,
// then subscribe to `processOutput` events for the spawned pid.
const { pid } = await vm.spawn("npm", ["run", "dev", "--prefix", "/home/agentos/sandbox/app"]);
const conn = vm.connect();
conn.on("processOutput", (payload) => {
	if (payload.pid === pid) {
		console.log(payload.stream, new TextDecoder().decode(payload.data));
	}
});
