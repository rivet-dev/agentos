// docs:start create
import { createVm } from "secure-exec";

// A VM lives until you dispose it. It is an agentOS VM with Secure Exec's
// defaults, so files, installed packages, and processes persist across calls.
const vm = await createVm();
// docs:end create

try {
	// docs:start files
	// Hand the guest a file from the host.
	await vm.filesystem.writeFile(
		"/workspace/orders.csv",
		"item,price\ncoffee,4\nbagel,3\n",
	);

	// Guest code uses ordinary `node:fs`. Each call gets fresh JavaScript memory
	// and the same filesystem.
	await vm.javascript.execute(`
		import { readFileSync, writeFileSync } from "node:fs";

		const rows = readFileSync("/workspace/orders.csv", "utf8").trim().split("\\n").slice(1);
		const total = rows.reduce((sum, row) => sum + Number(row.split(",")[1]), 0);
		writeFileSync("/workspace/report.json", JSON.stringify({ orders: rows.length, total }));
	`);

	// Read what it produced back on the host.
	const report = await vm.filesystem.readFile("/workspace/report.json");
	console.log(new TextDecoder().decode(report)); // {"orders":2,"total":7}
	// docs:end files

	// docs:start run-file
	// Run a file that is already in the VM, and type-check the project around it.
	await vm.filesystem.writeFile(
		"/workspace/main.ts",
		'const total: number = 42;\nconsole.log("total", total);\n',
	);
	const ran = await vm.typescript.executeFile("/workspace/main.ts", {
		output: { capture: "all" },
	});
	console.log(ran.stdout?.trim()); // total 42
	// docs:end run-file
} finally {
	// docs:start dispose
	await vm.dispose();
	// docs:end dispose
}
