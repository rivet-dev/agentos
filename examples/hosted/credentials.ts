import { createAgentOsClient } from "@rivet-dev/agentos";

export interface HostedCredentialIssuer {
	endpoint: string;
	namespace: string;
	poolName: string;
	adminToken: string;
}

// docs:start node-credential
/** Run on your backend; hand this credential only to your hosted worker. */
export async function issueHostedNodeCredential(
	options: HostedCredentialIssuer,
) {
	const issuer = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.adminToken,
	});
	try {
		return await issuer.auth.issueToken({
			subject: "agentos-worker",
			expiresIn: 600,
			grants: [
				{ resource: "node", target: "any", operations: ["create"] },
				// The worker creates its shared preload coordinator through the Gateway.
				{ resource: "actor", target: "any", operations: ["create", "read"] },
				{
					resource: "actor_gateway",
					target: "any",
					operations: ["create", "read"],
				},
			],
		});
	} finally {
		await issuer.dispose();
	}
}
// docs:end node-credential

// docs:start actor-credential
/** Call only after your backend has authorized this subject to own this VM. */
export async function issueHostedVmCredential(
	options: HostedCredentialIssuer,
	actorId: string,
	subject: string,
) {
	const issuer = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.adminToken,
	});
	try {
		// Default permissions grant gateway access to this Actor ID only.
		return await issuer.getForId("agentOS", actorId).issueToken({
			subject,
			expiresIn: 300,
		});
	} finally {
		await issuer.dispose();
	}
}
// docs:end actor-credential

// docs:start scoped-client
export async function readHostedVmStatus(options: {
	endpoint: string;
	namespace: string;
	poolName: string;
	actorId: string;
	token: string;
}) {
	const client = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.token,
	});
	try {
		// A gateway-only JWT accesses an existing ID; it cannot create a VM.
		return await client.getForId("agentOS", options.actorId).vm.status({});
	} finally {
		await client.dispose();
	}
}
// docs:end scoped-client
