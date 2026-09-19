import type { Permissions } from "@rivet-dev/agentos";
import { agentOS, setup } from "@rivet-dev/agentos";

// Allow the filesystem everywhere, but deny anything under /home/agentos/vault.
const denyVault = {
	fs: {
		default: "allow",
		rules: [
			{ mode: "deny", operations: ["*"], paths: ["/home/agentos/vault/**"] },
		],
	},
} satisfies Permissions;

// Deny the network by default, allow only api.example.com.
const allowOneHost = {
	network: {
		default: "deny",
		rules: [
			{
				mode: "allow",
				operations: ["*"],
				patterns: ["dns://api.example.com", "tcp://api.example.com:*"],
			},
		],
	},
} satisfies Permissions;

// Deny all host functions by default, then allow only "add" by name.
const allowOneHostFunction = {
	hostFunction: {
		default: "deny",
		rules: [{ mode: "allow", operations: ["invoke"], patterns: ["add"] }],
	},
} satisfies Permissions;

const vm = agentOS({
	permissions: {
		...denyVault,
		...allowOneHost,
		...allowOneHostFunction,
	},
});

export const registry = setup({ use: { vm } });
registry.start();
