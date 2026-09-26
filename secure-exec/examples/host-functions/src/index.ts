import { evaluate } from "secure-exec";
import { z } from "zod";

// docs:start quickstart
// docs:start call
// Inside the VM each collection is a global, and each function is async. This
// is the code a model would write.
const generated = `(async () => {
	const list = await orders.list({ customer: inputs.customer });
	return list.reduce((sum, order) => sum + order.amount, 0);
})()`;
// docs:end call

// docs:start define
// Host functions run in your process, with your credentials. The guest only
// sees their inputs and outputs. The keys name the collection and the function,
// and `execute` receives the input its own schema describes.
const total = await evaluate<number>(generated, {
	hostFunctions: {
		orders: {
			list: {
				inputSchema: z
					.object({ customer: z.string() })
					.describe("List a customer's orders."),
				execute: ({ customer }) => [
					{ customer, amount: 40 },
					{ customer, amount: 2 },
				],
			},
		},
	},
	inputs: { customer: "customer_123" },
	timeoutMs: 5_000,
	output: { capture: "stderr" },
});
console.log(total.outcome === "succeeded" ? total.value : total.stderr); // 42
// docs:end define
// docs:end quickstart
