import { createVm } from "secure-exec";

// docs:start install
// Packages install into a VM's filesystem, so they need a VM that outlives the
// call. Installing needs the network, which is denied unless you allow it.
const vm = await createVm({ permissions: { network: "allow" } });

try {
	const installed = await vm.npm.install(["zod"], {
		output: { capture: "all" },
	});
	if (installed.outcome !== "succeeded") {
		throw new Error(`npm install failed: ${installed.stderr}`);
	}
	// docs:end install

	// docs:start use
	// Packages install into the working directory, /workspace. A file there
	// resolves them the same way it would in Node.js.
	await vm.filesystem.writeFile(
		"/workspace/main.mjs",
		`
		import { z } from "zod";
		const parsed = z.object({ name: z.string() }).parse({ name: "secure-exec" });
		console.log(JSON.stringify(parsed));
		`,
	);
	const ran = await vm.javascript.executeFile("/workspace/main.mjs", {
		output: { capture: "all" },
	});
	console.log(ran.stdout?.trim()); // {"name":"secure-exec"}
	// docs:end use
} finally {
	await vm.dispose();
}
