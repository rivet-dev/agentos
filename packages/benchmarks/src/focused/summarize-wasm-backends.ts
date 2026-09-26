import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { summarizeBackendComparison } from "../lib/backend-summary.js";

// Recompute derived fields only. Never resolve or launch a sidecar here.
const [input, output, ...extra] = process.argv.slice(2);
if (!input || !output || extra.length || resolve(input) === resolve(output)) {
	throw new Error(
		"usage: tsx src/focused/summarize-wasm-backends.ts INPUT.json NEW_OUTPUT.json",
	);
}
const result = JSON.parse(readFileSync(input, "utf8"));
if (result.status !== "complete")
	throw new Error("summary requires a completed raw benchmark");
result.summary = summarizeBackendComparison(result);
// Exclusive creation also protects input aliases and existing artifacts.
writeFileSync(output, `${JSON.stringify(result, null, 2)}\n`, { flag: "wx" });
console.log(JSON.stringify(result.summary, null, 2));
