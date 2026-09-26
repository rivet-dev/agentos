import { evaluate } from "secure-exec";
import { z } from "zod";

// docs:start host-functions
// Each narrow capability the agent is allowed to use. The handlers run on the
// host with your credentials; the guest only ever sees validated inputs and
// JSON results. `.describe()` on the schema is what the model reads.
const store = {
	listOrders: {
		inputSchema: z
			.object({ customer: z.string() })
			.describe("List a customer's orders."),
		execute: ({ customer }: { customer: string }) => [
			{ id: "o_1", customer, total: 40, status: "shipped" },
			{ id: "o_2", customer, total: 2, status: "pending" },
		],
	},
	refund: {
		inputSchema: z
			.object({ orderId: z.string() })
			.describe("Refund one order."),
		execute: ({ orderId }: { orderId: string }) => ({
			orderId,
			refunded: true,
		}),
	},
};
// docs:end host-functions

// docs:start generated-code
// One program from the model instead of a tool call per step. It branches,
// loops, and combines results before anything comes back to you.
const generated = `
	const orders = await store.listOrders({ customer: inputs.customer });
	const pending = orders.filter((order) => order.status === "pending");
	for (const order of pending) {
		await store.refund({ orderId: order.id });
	}
	return { refunded: pending.length, total: pending.reduce((sum, o) => sum + o.total, 0) };
`;
// docs:end generated-code

// docs:start run
// Expose this as the agent's single code-execution tool. Permissions, limits,
// and the timeout apply to every program it writes.
const result = await evaluate<{ refunded: number; total: number }>(
	`(async () => {${generated}})()`,
	{
		hostFunctions: { store },
		inputs: { customer: "customer_123" },
		timeoutMs: 5_000,
		output: { capture: "stderr" },
	},
);
console.log(result.outcome === "succeeded" ? result.value : result.stderr);
// { refunded: 1, total: 2 }
// docs:end run
