import { AgentOs } from "@rivet-dev/agentos";

const vm = await AgentOs.create();

try {
	// Install a registry package while the VM is running. The bytes are verified
	// against the published digest before the package is projected into
	// /opt/agentos.
	const installed = await vm.software.install({
		type: "registry",
		name: "ripgrep",
	});
	console.log(installed.packageName, installed.version, installed.commands);

	const result = await vm.process.execFile("rg", ["--version"], {
		output: { capture: "all" },
	});
	console.log(result.stdout);

	// Packages installed through this API, and every command the VM provides.
	console.log(vm.software.installed());
	console.log(await vm.software.list());

	await vm.software.uninstall(installed.packageId);
} finally {
	await vm.dispose();
}
