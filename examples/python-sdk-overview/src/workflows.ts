import { PythonRuntime } from "@rivet-dev/agentos-python";

const python = await PythonRuntime.create();
try {
	await python.install(["requests==2.32.4"]);
	await python.install({ requirementsFile: "requirements.txt" });

	await python.executeFile("/workspace/report.py", { args: ["--json"] });
	await python.executeModule("http.server", {
		args: ["8000"],
		detached: true,
	});

	const asyncResult = await python.evaluate<string>(
		"await fetch_message()",
		{ executionId: "async", createIfMissing: true },
	);
	if (asyncResult.outcome !== "succeeded") {
		throw new Error(asyncResult.error.message);
	}
} finally {
	await python.dispose();
}
