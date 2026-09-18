// docs:start retain
import { createContext, evaluate, execute } from "secure-exec";
import { evaluate as evaluateTypeScript } from "secure-exec/typescript";

// A context is a dedicated VM that keeps state between calls, like a REPL.
// `await using` disposes the VM when the context goes out of scope.
await using context = await createContext();

await execute("globalThis.cart = []", { context });
await execute(`cart.push({ item: "coffee", price: 4 })`, { context });
await execute(`cart.push({ item: "bagel", price: 3 })`, { context });

// JavaScript and TypeScript share the same state.
const total = await evaluateTypeScript<number>(
	`(globalThis.cart as { price: number }[]).reduce((sum, line) => sum + line.price, 0)`,
	{ context },
);
console.log(total.outcome === "succeeded" ? total.value : total.error); // 7
// docs:end retain

// docs:start isolated
// Calls without the context run in their own VM and never see its state.
const isolated = await evaluate<string>("typeof globalThis.cart");
console.log(isolated.outcome === "succeeded" ? isolated.value : isolated.error); // undefined
// docs:end isolated

// docs:start reset
// `reset` clears the state and keeps the VM.
await context.reset();
const afterReset = await evaluate<string>("typeof globalThis.cart", {
	context,
});
console.log(
	afterReset.outcome === "succeeded" ? afterReset.value : afterReset.error,
); // undefined
// docs:end reset
