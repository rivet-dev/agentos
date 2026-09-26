import { createAgentOsClient, type Input } from "@rivet-dev/agentos";

type Permissions = NonNullable<Input.AgentOsActorConfigInput["permissions"]>;

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
			{ mode: "allow", operations: ["*"], patterns: ["api.example.com"] },
		],
	},
} satisfies Permissions;

const client = createAgentOsClient();
export const vm = client.agentOS.getOrCreate(
	["examples", "permissions", "combined"],
	{
		createWithInput: {
			config: {
				permissions: {
					...denyVault,
					...allowOneHost,
				},
			},
		},
	},
);
