import { createClient } from "@rivet-dev/agentos/client";
import type { registry } from "./server";

const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});

// The installed software determines which commands the VM can run. With grep
// available, this pipeline works inside the sandbox.
const result = await client.vm.getOrCreate("my-agent").exec("echo hello | grep hello");
console.log(result.stdout); // "hello\n"
