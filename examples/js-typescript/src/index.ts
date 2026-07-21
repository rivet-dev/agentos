import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const source = `
interface Greeting {
  name: string;
  count: number;
}

const greeting: Greeting = { name: "agentos", count: 3 };
for (let index = 0; index < greeting.count; index++) {
  console.log(\`hello \${greeting.name} #\${index + 1}\`);
}
`;

const runtime = await JavaScriptRuntime.create();
try {
	// Execution transpiles TypeScript without performing a semantic type check.
	const execution = await runtime.typescript.execute(source);
	if (execution.outcome !== "succeeded") throw new Error(execution.error.message);
	console.log(execution.stdout.trimEnd());

	// Type checking is explicit and reports structured diagnostics.
	const checked = await runtime.typescript.check(
		`const total: number = "not a number";`,
	);
	for (const diagnostic of checked.diagnostics) {
		console.log(
			`${diagnostic.category} TS${diagnostic.code}: ${diagnostic.message}`,
		);
	}
} finally {
	await runtime.dispose();
}
