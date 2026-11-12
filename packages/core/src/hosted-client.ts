import { createRequire } from "node:module";
import type { ClientConfigInput } from "rivetkit/client";
import type {
	AgentOsClient,
	AgentOsRegistry,
} from "./hosted/generated-contract.js";

const requireHostedTransport = createRequire(import.meta.url);

/** Connect to the native hosted agentOS actor using its generated contract. */
export function createAgentOsClient(
	endpointOrConfig?: string | ClientConfigInput,
): AgentOsClient {
	// RivetKit has an explicit CJS export. Resolve it only for hosted callers so
	// importing the embedded API does not initialize another client runtime.
	const { createClient } = requireHostedTransport(
		"rivetkit/client",
	) as typeof import("rivetkit/client");
	return createClient<AgentOsRegistry>(
		endpointOrConfig,
	) as unknown as AgentOsClient;
}

export type {
	AgentOsActions,
	AgentOsActorConnection,
	AgentOsActorCreateInput,
	AgentOsActorHandle,
	AgentOsClient,
	AgentOsEvents,
	AgentOsRegistry,
	AgentOsRivetError,
} from "./hosted/generated-contract.js";
export {
	AGENTOS_API_VERSION,
	AGENTOS_CAPABILITIES,
	AGENTOS_CONTRACT,
	AGENTOS_CONTRACT_HASH,
	AGENTOS_CONTRACT_MAJOR,
} from "./hosted/generated-contract.js";
