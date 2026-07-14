import { resolve } from "node:path";
import common from "@agentos-software/common";
import fd from "@agentos-software/fd";
import git from "@agentos-software/git";
import pi from "@agentos-software/pi";
import ripgrep from "@agentos-software/ripgrep";
import vim from "@agentos-software/vim";
import { agentOS, setup } from "@rivet-dev/agentos";

// Rivet engines intentionally outlive their host process. Keep this demo's
// RocksDB state workspace-local so another checkout's engine cannot hold its
// database lock and prevent this one from starting.
process.env.RIVETKIT_STORAGE_PATH ??= resolve(
	import.meta.dirname,
	".rivetkit-data",
);

const shellVm = agentOS({
	software: [common, fd, ripgrep, git, vim, pi],
	permissions: {
		network: {
			default: "deny",
			rules: [
				{
					mode: "allow",
					operations: ["http"],
					patterns: ["tcp://127.0.0.1:6431"],
				},
				{
					// Guest TCP clients bind a loopback ephemeral port before connect.
					mode: "allow",
					operations: ["listen"],
					patterns: ["tcp://127.0.0.1:*"],
				},
			],
		},
	},
});

export const registry = setup({ use: { shellVm } });

registry.start();
