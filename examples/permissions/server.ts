import type { Permissions } from "@rivet-dev/agentos";
import { agentOS, setup } from "@rivet-dev/agentos";

// docs:start grant-network
// Grant the network, leave everything else at the secure default.
const grantNetwork = { network: "allow" } satisfies Permissions;
// docs:end grant-network

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
			{
				mode: "allow",
				operations: ["*"],
				patterns: ["dns://api.example.com", "tcp://api.example.com:*"],
			},
		],
	},
} satisfies Permissions;
// docs:end allow-one-host

// docs:start allow-one-host-function
// Deny all host functions by default, then allow only "add" by name.
const allowOneHostFunction = {
	hostFunction: {
		default: "deny",
		rules: [{ mode: "allow", operations: ["invoke"], patterns: ["add"] }],
	},
} satisfies Permissions;
// docs:end allow-one-host-function

// Combine the policies above and bind them to the VM via `agentOS`.
const vm = agentOS({
	permissions: {
		...grantNetwork,
		...denyVault,
		...allowOneHost,
		...allowOneHostFunction,
	},
});

export const registry = setup({ use: { vm } });
registry.start();
