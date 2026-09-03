import { vm } from "./actor.js";

const sourceUrl = process.env.AGENTOS_PACKAGE_URL;
if (!sourceUrl) {
	throw new Error("set AGENTOS_PACKAGE_URL to an immutable .aospkg URL");
}

const installed = await vm.software.install({
	source: {
		url: sourceUrl,
		digest: process.env.AGENTOS_PACKAGE_DIGEST,
	},
});
console.log("configured:", installed.source.packageId);
if (installed.config.state === "restart_required") {
	await vm.vm.restart({});
}

// `rg` (ripgrep) and `jq` are now available inside the VM. Find files containing
// "TODO" and pretty-print the matching paths as JSON.
const result = await vm.process.run({
	command: "sh",
	args: ["-c", "rg --files-with-matches TODO /home/agentos | jq -R ."],
	options: { env: {}, captureStdio: true },
});
console.log(result.stdout);
