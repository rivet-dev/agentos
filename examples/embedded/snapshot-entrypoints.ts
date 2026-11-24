import { AgentOs } from "@rivet-dev/agentos";

// The runtime bundle lives on the host and is mounted read-only. Its thin
// entry (`main.mjs`) imports the runtime and starts it; everything it imports
// is evaluated once into a V8 snapshot instead of once per process.
const vm = await AgentOs.create({
	mounts: [
		{
			path: "/opt/runtime",
			readOnly: true,
			plugin: {
				id: "host_dir",
				config: { hostPath: "/srv/my-runtime/dist", readOnly: true },
			},
		},
	],
	snapshotEntrypoints: ["/opt/runtime/main.mjs"],
});

try {
	// The first processes load normally while the snapshot builds in the
	// background. Later processes start with the runtime already evaluated.
	const { pid } = await vm.process.spawn("node", ["/opt/runtime/main.mjs"], {
		onStdout: (chunk) => process.stdout.write(chunk),
	});
	await vm.process.wait(pid);
} finally {
	await vm.dispose();
}
