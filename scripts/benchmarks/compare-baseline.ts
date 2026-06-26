import { existsSync } from "node:fs";
import { compareBaseline } from "./lib/report.js";

export function compareBaselineFile(currentPath: string, baselinePath: string) {
	if (!existsSync(baselinePath)) {
		return {
			status: "missing-baseline",
			baselinePath,
			regressions: [],
		};
	}
	const rows = compareBaseline(currentPath, baselinePath);
	return {
		status: rows.some((row) => row.regressed) ? "regressed" : "ok",
		regressions: rows.filter((row) => row.regressed),
		rows,
	};
}

if (import.meta.url === `file://${process.argv[1]}`) {
	const current = process.argv[2] ?? "scripts/benchmarks/results/findings.json";
	const baseline =
		process.argv[3] ??
		"scripts/benchmarks/results/baseline/findings-baseline.json";
	console.log(JSON.stringify(compareBaselineFile(current, baseline), null, 2));
}
