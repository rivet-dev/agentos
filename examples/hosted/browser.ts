import {
	type AgentOsActorCreateInput,
	createAgentOsClient,
} from "@rivet-dev/agentos";
import type { HostedQuickstartOptions } from "./quickstart.js";

export async function runBrowserCommand(
	options: HostedQuickstartOptions,
	source: { url: string; digest: string },
	args: string[],
	env: Record<string, string>,
	permissions: NonNullable<
		NonNullable<AgentOsActorCreateInput["config"]>["permissions"]
	>,
) {
	if (new URL(source.url).protocol !== "https:")
		throw new Error("Use an HTTPS package URL");
	const client = createAgentOsClient({
		endpoint: options.endpoint,
		namespace: options.namespace,
		poolName: options.poolName,
		token: options.token,
	});
	try {
		// Use a fresh actor key so its creation config installs this package.
		const vm = client.agentOS.getOrCreate([options.key], {
			createWithInput: { config: { software: [source], permissions } },
		});
		return await vm.process.run({
			command: "browse",
			args,
			options: { env, captureStdio: true, timeoutMs: 30_000 },
		});
	} finally {
		await client.dispose();
	}
}
