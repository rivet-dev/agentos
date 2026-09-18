// docs:start parallel
import { createVm } from "secure-exec";

const vm = await createVm();

// A context runs one call at a time. For parallel work, create several: each
// has its own JavaScript memory, and they share the VM's files and packages.
const shards = await Promise.all([
	vm.createContext(),
	vm.createContext(),
	vm.createContext(),
]);

const started = performance.now();
const results = await Promise.all(
	shards.map((context, shard) =>
		context.evaluate<number>(
			"new Promise((resolve) => setTimeout(() => resolve(inputs.shard * 10), 500))",
			{ inputs: { shard } },
		),
	),
);

console.log(results.map((result) => ("value" in result ? result.value : null))); // [0, 10, 20]
console.log(`${Math.round(performance.now() - started)}ms`); // well under the 1500ms that running them one after another takes

await vm.dispose();
// docs:end parallel
