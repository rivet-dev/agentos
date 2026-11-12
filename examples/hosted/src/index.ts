import { runHostedQuickstart } from "../quickstart.js";

function required(name: string): string {
	const value = process.env[name];
	if (!value) throw new Error(`Set ${name} before running the hosted example`);
	return value;
}

const result = await runHostedQuickstart({
	endpoint: required("RIVET_ENDPOINT"),
	namespace: required("RIVET_NAMESPACE"),
	poolName: required("RIVET_POOL"),
	token: required("RIVET_TOKEN"),
	key: process.env.AGENTOS_VM_KEY ?? "hosted-example",
});
console.log(result.stdout);
