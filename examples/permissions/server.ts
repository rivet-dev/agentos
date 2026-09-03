import { createAgentOsClient, type Input } from "@rivet-dev/agentos";

type Permissions = NonNullable<Input.AgentOsActorConfigInput["permissions"]>;

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
			{ mode: "allow", operations: ["*"], patterns: ["api.example.com"] },
		],
	},
} satisfies Permissions;
// docs:end allow-one-host

const client = createAgentOsClient();
export const vm = client.agentOS.getOrCreate(["examples", "permissions"], {
	createWithInput: {
		config: {
			permissions: {
				...grantNetwork,
				...denyVault,
				...allowOneHost,
			},
		},
	},
});

console.log(await vm.vm.status({}));
