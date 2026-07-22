import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});
const vm = client.vm.getOrCreate("my-agent");

// Write code via the filesystem. The /home/agentos/sandbox mount maps to the sandbox root.
await vm.writeFile(
	"/home/agentos/sandbox/app/index.ts",
	'console.log("hello")',
);

// Run it inside the sandbox through the generated binding command.
// The VM path above maps to /app/index.ts at the sandbox root.
const result = await vm.exec(
	"agentos-sandbox run-command --command node --args /app/index.ts",
);
console.log(result.stdout); // "hello\n"

const install = await vm.exec(
	"agentos-sandbox run-command --command npm --args install --args --prefix --args /app",
);
console.log(install.exitCode, install.stdout);
