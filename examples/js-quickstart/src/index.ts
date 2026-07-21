import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

// docs:start vanilla-javascript
// Boot a fully virtualized runtime. Guest code runs inside the kernel
// isolation boundary - no host escapes.
const runtime = await JavaScriptRuntime.create();

try {
	// evaluate() returns a JSON-serializable value and captures stdout/stderr.
	const result = await runtime.evaluate<{ message: string; sum: number }>(`
		(() => {
			console.log("hello from AgentOS");
			return { message: "hello from AgentOS", sum: 1 + 2 };
		})()
	`);

	if (result.outcome !== "succeeded") throw new Error(result.error.message);
	console.log("stdout:", JSON.stringify(result.stdout.trim()));
	console.log("value:", result.value);
	console.log("exitCode:", result.exitCode);
} finally {
	// Tear down the VM and release the sidecar.
	await runtime.dispose();
}
// docs:end vanilla-javascript

// docs:start typescript
const typedRuntime = await JavaScriptRuntime.create();
try {
	const typed = await typedRuntime.typescript.execute(`
		const answer: number = 21 * 2;
		console.log(answer);
	`);
	if (typed.outcome !== "succeeded") throw new Error(typed.error.message);
} finally {
	await typedRuntime.dispose();
}
// docs:end typescript
