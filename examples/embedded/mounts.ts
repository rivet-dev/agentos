import { AgentOs } from "@rivet-dev/agentos";

// Configure filesystem backends at boot. Native mount plugins (host
// directories, S3, Google Drive, etc.) are passed via `plugin`, each identified
// by an `id` and a `config` object.
const vm = await AgentOs.create({
	mounts: [
		// Host directory (read-only)
		{
			path: "/mnt/code",
			plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
			readOnly: true,
		},
		// S3 bucket
		{
			path: "/mnt/data",
			plugin: {
				id: "chunked_s3",
				config: { bucket: "my-bucket", prefix: "agent/", region: "us-east-1" },
			},
		},
	],
});

await vm.dispose();
