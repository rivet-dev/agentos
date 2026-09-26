import { execute } from "secure-exec";

// docs:start execute
// `execute` runs a whole ES module for its side effects. Capture output to read
// it back.
const run = await execute(
	`
	import { platform } from "node:os";
	console.log("running on", platform());
	`,
	{ output: { capture: "all" } },
);
console.log(run.stdout?.trim()); // running on linux
// docs:end execute
