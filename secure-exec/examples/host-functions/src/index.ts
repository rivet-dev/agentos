import { evaluate, hostFunction, hostFunctions } from "secure-exec";
import { z } from "zod";

// docs:start define
// Host functions run in your process, with your credentials. The guest only
// sees their inputs and outputs.
const orders = hostFunctions({
	name: "orders",
	description: "Look up customer orders.",
	functions: {
		list: hostFunction({
			description: "List a customer's orders.",
			inputSchema: z.object({ customer: z.string() }),
			execute: ({ customer }) => [
				{ customer, amount: 40 },
				{ customer, amount: 2 },
			],
		}),
	},
});
// docs:end define

// docs:start call
// Inside the VM each collection is a global, and each function is async. This
// is the code a model would write.
const generated = `(async () => {
	const list = await orders.list({ customer: inputs.customer });
	return list.reduce((sum, order) => sum + order.amount, 0);
})()`;

const total = await evaluate<number>(generated, {
	hostFunctions: [orders],
	inputs: { customer: "customer_123" },
	timeoutMs: 5_000,
	output: { capture: "stderr" },
});
console.log(total.outcome === "succeeded" ? total.value : total.stderr); // 42
// docs:end call
