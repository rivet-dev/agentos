import { AgentOs } from "@rivet-dev/agentos";

// The default bundle (sh, coreutils, and the standard CLI tools) is installed
// first, then the packages listed here. Registry packages resolve from this
// client's release and are verified against their published digest.
const vm = await AgentOs.create({
	software: [{ registry: "git" }],
});

try {
	const result = await vm.process.exec("git --version", {
		output: { capture: "all" },
	});
	console.log(result.stdout);
} finally {
	await vm.dispose();
}
