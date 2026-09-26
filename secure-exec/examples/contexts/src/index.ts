// docs:start retain
import { createVm } from "secure-exec";

const vm = await createVm();

// A context keeps JavaScript variables and imports between calls, like a REPL.
const context = await vm.createContext();

await context.execute("globalThis.cart = []");
await context.execute(`cart.push({ item: "coffee", price: 4 })`);
await context.execute(`cart.push({ item: "bagel", price: 3 })`);

const total = await context.evaluate<number>(
	"cart.reduce((sum, line) => sum + line.price, 0)",
);
console.log(total.outcome === "succeeded" ? total.value : total.error); // 7
// docs:end retain

// docs:start typescript
// TypeScript shares the same state. Pass the context's id to `vm.typescript`.
const typed = await vm.typescript.evaluate<number>(
	"(globalThis.cart as { price: number }[]).length",
	{ contextId: context.contextId },
);
console.log(typed.outcome === "succeeded" ? typed.value : typed.error); // 2
// docs:end typescript

// docs:start isolated
// Calls outside the context never see its state, even in the same VM.
const outside = await vm.javascript.evaluate<string>("typeof globalThis.cart");
console.log(outside.outcome === "succeeded" ? outside.value : outside.error); // undefined
// docs:end isolated

// docs:start reset
// `reset` clears the state. `dispose` deletes the context and leaves the VM.
await context.reset();
const afterReset = await context.evaluate<string>("typeof globalThis.cart");
console.log(
	afterReset.outcome === "succeeded" ? afterReset.value : afterReset.error,
); // undefined

await context.dispose();
await vm.dispose();
// docs:end reset
