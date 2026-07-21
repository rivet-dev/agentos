import { JavaScriptRuntime } from "@rivet-dev/agentos-javascript";

const runtimeA = await JavaScriptRuntime.create();
const runtimeB = await JavaScriptRuntime.create();

try {
	await runtimeA.execute(`
		import { writeFileSync } from "node:fs";
		writeFileSync("/tmp/value.txt", "A");
	`);
	await runtimeB.execute(`
		import { writeFileSync } from "node:fs";
		writeFileSync("/tmp/value.txt", "B");
	`);

	const read = `(async () => {
		const { readFile } = await import("node:fs/promises");
		return readFile("/tmp/value.txt", "utf8");
	})()`;
	const [valueA, valueB] = await Promise.all([
		runtimeA.evaluate<string>(read),
		runtimeB.evaluate<string>(read),
	]);
	console.log({ valueA, valueB });

	// Each evaluate() launches a fresh process, so guest globals do not persist.
	const globalValue = `(() => {
		globalThis.counter = (globalThis.counter ?? 0) + 1;
		return globalThis.counter;
	})()`;
	console.log(await runtimeA.evaluate<number>(globalValue));
	console.log(await runtimeA.evaluate<number>(globalValue));
} finally {
	await runtimeA.dispose();
	await runtimeB.dispose();
}
