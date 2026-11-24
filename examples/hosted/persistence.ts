import { createAgentOsClient } from "@rivet-dev/agentos";

export interface HostedPersistenceOptions {
	endpoint: string;
	namespace: string;
	poolName: string;
	token: string;
	key: string;
}

export async function verifyHostedPersistence(
	options: HostedPersistenceOptions,
) {
	const client = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.token,
	});
	try {
		const vm = client.agentOS.getOrCreate([options.key], {
			createWithInput: {
				config: {
					filesystem: {
						mounts: [
							{
								path: "/workspace",
								backend: { type: "actor-sqlite" },
								readOnly: false,
							},
						],
					},
				},
			},
		});
		await vm.filesystem.writeFile({
			path: "/workspace/hello.txt",
			content: "Hello from agentOS!",
		});
		await vm.vm.restart({});
		const bytes = await vm.filesystem.readFile({
			path: "/workspace/hello.txt",
			maxBytes: 1024,
		});
		const text = new TextDecoder().decode(bytes);
		if (text !== "Hello from agentOS!")
			throw new Error("Durable file did not survive restart");
		return text;
	} finally {
		await client.dispose();
	}
}
