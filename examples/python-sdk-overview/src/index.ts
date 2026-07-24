// docs:start overview
// docs:start imports
import { PythonRuntime } from "@rivet-dev/agentos-python";

// docs:end imports

const python = await PythonRuntime.create({
	cwd: "/workspace",
	env: { GREETING: "hello from the VM" },
});

try {
	const execution = await python.execute(`
import os
import sys

print(os.environ["GREETING"])
print("stderr is captured separately", file=sys.stderr)
	`);
	console.log(execution);

	const evaluation = await python.evaluate<{ sum: number; cwd: string }>(
		`{"sum": 2 + 40, "cwd": __import__("os").getcwd()}`,
	);
	console.log(evaluation);
} finally {
	await python.dispose();
}
// docs:end overview
