import { fileURLToPath } from "node:url";
import {
	execute,
	executeFile,
	hostDirMount,
	nodeModulesMount,
} from "secure-exec";

// docs:start host-dir
// Project one host directory into the VM, read-only. The guest sees only the
// mounted subtree, never the rest of the host.
const hostData = fileURLToPath(new URL("../host-data", import.meta.url));

const read = await execute(
	`
	import { readFileSync } from "node:fs";
	console.log(readFileSync("/mnt/data/greeting.txt", "utf8").trim());
	`,
	{
		mounts: [hostDirMount("/mnt/data", hostData)],
		output: { capture: "all" },
	},
);
console.log(read.stdout?.trim()); // hello from the host
// docs:end host-dir

// docs:start node-modules
// Mount a host directory of packages as the guest's node_modules, so code can
// import packages you already have with no network and no install step. Point it
// at your project's `node_modules` in a real app.
const hostModules = fileURLToPath(new URL("../host-modules", import.meta.url));

const imported = await execute(
	`
	import { greet } from "greet";
	console.log(greet("secure-exec"));
	`,
	{
		mounts: [nodeModulesMount(hostModules)],
		output: { capture: "all" },
	},
);
console.log(imported.stdout?.trim()); // hello, secure-exec
// docs:end node-modules

// docs:start execute-file
// `executeFile` runs a file that is already in the VM. With a mount, that works
// in a one-shot call: the script lives on the host and runs inside the VM.
const report = await executeFile("/mnt/data/report.mjs", {
	mounts: [hostDirMount("/mnt/data", hostData)],
	output: { capture: "all" },
});
console.log(report.stdout?.trim()); // report: hello from the host
// docs:end execute-file
