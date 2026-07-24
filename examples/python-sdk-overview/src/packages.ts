import { PythonRuntime } from "@rivet-dev/agentos-python";

const runtime = await PythonRuntime.create();
try {
	const installed = await runtime.install(["requests"], { upgrade: true });
	if (installed.outcome !== "succeeded") throw new Error(installed.error.message);

	const result = await runtime.execute(`
import requests
print(requests.__version__)
	`);
	if (result.outcome !== "succeeded") throw new Error(result.error.message);
} finally {
	await runtime.dispose();
}
