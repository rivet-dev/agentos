import type { AgentOsSandboxProvider } from "@rivet-dev/agentos-core";
import { SandboxAgent } from "sandbox-agent";
import type { DockerProviderOptions } from "sandbox-agent/docker";
import { docker as sandboxAgentDocker } from "sandbox-agent/docker";

export type {
	SandboxFsOptions,
	SandboxMountPluginConfig,
} from "./mount.js";
export { createSandboxFs } from "./mount.js";

export type { SandboxBindingsOptions } from "./bindings.js";
export { createSandboxBindings } from "./bindings.js";

export type {
	AgentOsSandboxClient as SandboxClient,
	AgentOsSandboxClientOptions as SandboxClientOptions,
	AgentOsSandboxInput as SandboxInput,
	AgentOsSandboxOptions as SandboxOptions,
	AgentOsSandboxProvider as SandboxProvider,
	AgentOsSandboxProviderOptions as SandboxProviderOptions,
} from "@rivet-dev/agentos-core";
export type { DockerProviderOptions };

export function docker(options?: DockerProviderOptions): AgentOsSandboxProvider {
	const provider = sandboxAgentDocker(options);
	return {
		start: () => SandboxAgent.start({ sandbox: provider }),
	};
}
