import { agentOS, setup } from "@rivet-dev/agentos";

// Grant the network, leave everything else at the secure default.
const grantNetwork = { network: "allow" };

// fs: allow by default, but deny anything under /vault.
const denyVault = {
	fs: {
		default: "allow",
		rules: [{ mode: "deny", operations: ["*"], paths: ["/vault/**"] }],
	},
};

// network: deny by default, allow only api.example.com.
const allowOneHost = {
	network: {
		default: "deny",
		rules: [{ mode: "allow", operations: ["*"], patterns: ["api.example.com"] }],
	},
};

// binding: deny by default, allow only the "add" binding by name.
const allowOneBinding = {
	binding: {
		default: "deny",
		rules: [{ mode: "allow", operations: ["*"], patterns: ["add"] }],
	},
};

// Combine the policies above and bind them to the VM via `agentOS`.
const vm = agentOS({
	permissions: {
		...grantNetwork,
		...denyVault,
		...allowOneHost,
		...allowOneBinding,
	},
});

export const registry = setup({ use: { vm } });
registry.start();
