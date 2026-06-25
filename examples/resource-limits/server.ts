import { agentOS, setup } from "@rivet-dev/agentos";
import pi from "./software/pi";

const vm = agentOS({
	software: [pi],
	limits: {
		resources: {
			maxProcesses: 64, // concurrent processes
			maxOpenFds: 256, // open file descriptors
			maxSockets: 128, // open sockets
			maxFilesystemBytes: 256 * 1024 * 1024, // VFS storage budget
			maxWasmStackBytes: 4 * 1024 * 1024, // WASM call-stack ceiling
		},
	},
});

export const registry = setup({ use: { vm } });
registry.start();
