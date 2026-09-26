import { createAgentOsClient, type Input } from "@rivet-dev/agentos";

type Permissions = NonNullable<Input.AgentOsActorConfigInput["permissions"]>;

// docs:start deny-vault
// Allow the filesystem everywhere, but deny anything under /home/agentos/vault.
const denyVault = {
	fs: {
		default: "allow",
		rules: [
			{ mode: "deny", operations: ["*"], paths: ["/home/agentos/vault/**"] },
		],
	},
} satisfies Permissions;
// docs:end deny-vault

// docs:start allow-only-data
// Deny the filesystem by default, allow only reads under /home/agentos/data.
const allowOnlyData = {
	fs: {
		default: "deny",
		rules: [
			{
				mode: "allow",
				operations: ["read", "readdir", "stat"],
				paths: ["/home/agentos/data/**"],
			},
		],
	},
} satisfies Permissions;
// docs:end allow-only-data

const client = createAgentOsClient();
export const vm = client.agentOS.getOrCreate(
	["examples", "permissions", "filesystem"],
	{
		createWithInput: {
			config: {
				permissions: {
					...denyVault,
					...allowOnlyData,
				},
			},
		},
	},
);
