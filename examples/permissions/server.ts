import { agentOS, setup } from "@rivet-dev/agentos";
import type { Permissions } from "@rivet-dev/agentos-core";

// fs: allow by default, but deny anything under /vault.
const denyVault = {
	fs: {
		default: "allow",
		rules: [{ mode: "deny", operations: ["*"], paths: ["/vault/**"] }],
	},
} satisfies Permissions;

// docs:start allow-one-host
// Deny the network by default, allow only api.example.com.
const allowOneHost = {
	network: {
		default: "deny",
		rules: [
			{ mode: "allow", operations: ["*"], patterns: ["api.example.com"] },
		],
	},
} satisfies Permissions;
// docs:end allow-one-host

// docs:start allow-one-binding
// Deny all bindings by default, allow only the "add" binding by name.
const allowOneBinding = {
	binding: {
		default: "deny",
		rules: [{ mode: "allow", operations: ["*"], patterns: ["add"] }],
	},
} satisfies Permissions;
// docs:end allow-one-binding

// Combine the policies above and bind them to the VM via `agentOS`.
const vm = agentOS({
	permissions: {
		...denyVault,
		...allowOneHost,
		...allowOneBinding,
	},
});

export const registry = setup({ use: { vm } });
registry.start();
