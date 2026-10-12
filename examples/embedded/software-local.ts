import { resolve } from "node:path";
import { AgentOs } from "@rivet-dev/agentos";

// A trusted .aospkg file on the host, such as one you built with
// agentos-toolchain or downloaded from the registry.
const packagePath = resolve(process.argv[2] ?? "dist/package.aospkg");

// Install it when the VM is created.
const vm = await AgentOs.create({ software: [{ packagePath }] });
try {
	console.log(await vm.software.list());
} finally {
	await vm.dispose();
}

// Or install it into a running VM.
const running = await AgentOs.create();
try {
	const installed = await running.software.install({
		type: "path",
		path: packagePath,
	});
	console.log(installed.packageName, installed.version, installed.commands);
} finally {
	await running.dispose();
}
