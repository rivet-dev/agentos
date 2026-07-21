import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const rt = await JavaScriptRuntime.create();

try {
	const { stdout, stderr, exitCode } = await rt.execute(`
		console.log("hello from the VM");
		console.error("oops from the VM");
		process.exit(3);
	`);

	console.log("exitCode:", exitCode);
	console.log("stdout:", JSON.stringify(stdout));
	console.log("stderr:", JSON.stringify(stderr));
} finally {
	await rt.dispose();
}
