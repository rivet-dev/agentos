import { check, evaluate } from "secure-exec/typescript";

// Running TypeScript strips types without checking them, so type-check an
// agent's generated code first and feed the diagnostics back to it.
const generated = `([5, 10, "27"] as number[]).reduce((a, b) => a + b, 0)`;

const checked = await check(generated, { filePath: "generated.ts" });
for (const diagnostic of checked.diagnostics) {
	console.log(
		`${diagnostic.category} TS${diagnostic.code}: ${diagnostic.message}`,
	);
}

const fixed = generated.replace(`"27"`, "27");
const recheck = await check(fixed, { filePath: "generated.ts" });
if (recheck.outcome === "succeeded" && !recheck.hasErrors) {
	const result = await evaluate<number>(fixed);
	console.log(result.outcome === "succeeded" ? result.value : result.error); // 42
}
