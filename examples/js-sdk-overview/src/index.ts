// docs:start imports
import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";
// docs:end imports

const javascript = await JavaScriptRuntime.create({
	cwd: "/workspace",
	env: { GREETING: "hello from the VM" },
});

try {
	const execution = await javascript.execute(`
		console.log(process.env.GREETING);
		console.error("stderr is captured separately");
	`);
	console.log(execution);

	const evaluation = await javascript.evaluate<{ sum: number; cwd: string }>(
		`({ sum: 2 + 40, cwd: process.cwd() })`,
	);
	console.log(evaluation);

	const typed = await javascript.typescript.evaluate<number>(
		`Promise.resolve(21 * 2 satisfies number)`,
	);
	console.log(typed);
} finally {
	await javascript.dispose();
}
