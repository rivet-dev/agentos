import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const rt = await JavaScriptRuntime.create();

try {
	// A normal program finishes well within the timeout budget.
	const ok = await rt.execute(`console.log("finished work");`, {
		timeoutMs: 5000,
	});
	console.log("normal run:");
	console.log("  exitCode:", ok.exitCode);
	console.log("  stdout:", JSON.stringify(ok.stdout.trim()));

	// A runaway program (infinite loop) never returns on its own. The exec
	// timeout terminates the guest process after the budget elapses.
	const start = Date.now();
	const runaway = await rt.execute(`while (true) {}`, {
		timeoutMs: 1000,
	});
	const elapsed = Date.now() - start;

	console.log("runaway run (timeoutMs: 1000ms):");
	console.log("  exitCode:", runaway.exitCode);
	console.log("  elapsedMs:", elapsed);

	// A killed process exits non-zero; a clean exit would be 0.
	const terminated = runaway.exitCode !== 0;
	console.log(
		terminated
			? "runaway guest was terminated by the timeout"
			: "ERROR: runaway guest was NOT terminated",
	);

	if (!terminated) {
		process.exitCode = 1;
	}
} finally {
	await rt.dispose();
}
