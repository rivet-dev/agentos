// Advanced sandbox configuration: custom mount path and manual disposal.

import { AgentOs } from "@rivet-dev/agentos-core";
import { SandboxAgent } from "sandbox-agent";
import { docker } from "sandbox-agent/docker";

const sandbox = await SandboxAgent.start({ sandbox: docker() });

const vm = await AgentOs.create({
	sandbox: {
		client: sandbox,
		mountPath: "/work",
		dispose: false,
	},
});

try {
	await vm.writeFile("/work/hello.txt", "Hello from /work");
	console.log(new TextDecoder().decode(await vm.readFile("/work/hello.txt")));
} finally {
	await vm.dispose();
	await sandbox.dispose();
}
