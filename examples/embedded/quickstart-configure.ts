import { resolve } from "node:path";
import { AgentOs } from "@rivet-dev/agentos";

const vm = await AgentOs.create({
	// Store the root filesystem in SQLite so files survive VM recreation.
	database: { type: "sqlite_file", path: resolve("agentos.sqlite") },
	rootFilesystem: { type: "native", plugin: { id: "chunked_sqlite", config: {} } },

	// Attach a host directory (read-only) and an S3 bucket.
	mounts: [
		{
			path: "/mnt/code",
			plugin: { id: "host_dir", config: { hostPath: "/path/to/repo" } },
			readOnly: true,
		},
		{
			path: "/mnt/data",
			plugin: { id: "chunked_s3", config: { bucket: "my-bucket", region: "us-east-1" } },
		},
	],

	// Environment variables for guest processes.
	environment: { NODE_ENV: "production" },

	// Deny network access except to one host.
	permissions: {
		network: {
			default: "deny",
			rules: [
				{
					mode: "allow",
					operations: ["*"],
					patterns: ["dns://api.example.com", "tcp://api.example.com:*"],
				},
			],
		},
	},

	// Cap processes and filesystem size.
	limits: { resources: { maxProcesses: 64, maxFilesystemBytes: 256 * 1024 * 1024 } },
});

await vm.dispose();
