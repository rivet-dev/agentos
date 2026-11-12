// docs:start quickstart
import { createAgentOsClient } from "@rivet-dev/agentos";

export interface HostedQuickstartOptions {
	endpoint: string;
	namespace: string;
	poolName: string;
	token: string;
	key: string;
}

export async function runHostedQuickstart(options: HostedQuickstartOptions) {
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
		const result = await vm.process.run({
			command: "cat",
			args: ["/workspace/hello.txt"],
			options: { captureStdio: true, timeoutMs: 10_000 },
		});
		return { vmId: await vm.resolve(), stdout: result.stdout };
	} finally {
		await client.dispose();
	}
}
// docs:end quickstart

export async function runHostedPreview(options: HostedQuickstartOptions) {
	const client = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.token,
	});
	try {
		const vm = client.agentOS.getOrCreate([options.key]);
		await vm.filesystem.writeFile({
			path: "/home/agentos/server.js",
			content:
				'require("node:http").createServer((_req, res) => res.end("Hello from the VM!")).listen(3000);',
		});
		const spawned = await vm.process.spawn({
			command: "node",
			args: ["/home/agentos/server.js"],
		});
		try {
			const preview = await vm.network.preview.create({
				port: 3000,
				ttlMs: 60 * 60 * 1_000,
			});
			return { process: spawned, path: preview.path };
		} catch (error) {
			await vm.process.signal({ process: spawned, signal: "SIGTERM" });
			throw error;
		}
	} finally {
		await client.dispose();
	}
}
