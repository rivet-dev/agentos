import { PythonRuntime } from "@rivet-dev/agentos-python";

// docs:start python
const runtime = await PythonRuntime.create();

try {
	const result = await runtime.evaluate<number>("21 * 2");
	console.log(result.outcome === "succeeded" ? result.value : result.error);
} finally {
	await runtime.dispose();
}
// docs:end python
