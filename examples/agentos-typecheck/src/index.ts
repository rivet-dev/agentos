/**
 * Type-checking example for `@rivet-dev/agentos`.
 *
 * This file exercises the public native actor package surface. It is not meant
 * to run: the actual action dispatch lives in the Rust native actor plugin.
 */

import {
	type AgentOSConfigInput,
	type AgentOsEvents,
	agentOS,
	type NodeModulesMountConfig,
	nodeModulesMount,
	type PersistedSessionEvent,
	type PersistedSessionRecord,
	type PromptResult,
	type SerializableCronJobInfo,
	type SerializableCronJobOptions,
	type SessionRecord,
	setup,
} from "@rivet-dev/agentos";
import { createClient } from "@rivet-dev/agentos/client";
import pi from "./software/pi";

const mount: NodeModulesMountConfig = nodeModulesMount(
	"/abs/host/node_modules",
	{
		readOnly: true,
	},
);

const config: AgentOSConfigInput = {
	software: [pi],
	mounts: [mount],
	additionalInstructions: "Be concise.",
	allowedNodeBuiltins: ["path", "fs"],
	loopbackExemptPorts: [3000],
	preview: {
		defaultExpiresInSeconds: 3600,
		maxExpiresInSeconds: 86_400,
	},
	onSessionEvent: async (sessionId, event) => {
		console.log(sessionId, event.method);
	},
	onPermissionRequest: async (sessionId, request) => {
		console.log(sessionId, request.permissionId);
	},
};

const vm = agentOS(config);
const registry = setup({ use: { vm } });
const client = createClient<typeof registry>({
	endpoint: "http://localhost:6420",
});

type PublicDomainTypes =
	| PersistedSessionEvent
	| PersistedSessionRecord
	| PromptResult
	| SerializableCronJobInfo
	| SessionRecord;

function acceptPublicDomainType(value: PublicDomainTypes): PublicDomainTypes {
	return value;
}

function acceptEvent<K extends keyof AgentOsEvents>(
	name: K,
	payload: AgentOsEvents[K],
): AgentOsEvents[K] {
	console.log(name);
	return payload;
}

const cron: SerializableCronJobOptions = {
	schedule: "*/5 * * * *",
	action: { type: "exec", command: "echo", args: ["tick"] },
	overlap: "skip",
};

async function main(): Promise<void> {
	const handle = client.vm.getOrCreate("my-agent");

	await handle.createSession("pi", { cwd: "/work" });
	await handle.sendPrompt("session-1", "List the files in /work.");
	await handle.scheduleCron(cron);
	await handle.createSignedPreviewUrl(3000, 300);

	acceptEvent("sessionEvent", {
		sessionId: "session-1",
		event: { jsonrpc: "2.0", method: "session/update" },
	});
	acceptEvent("permissionRequest", {
		sessionId: "session-1",
		request: { permissionId: "perm-1", params: {} },
	});

	acceptPublicDomainType({
		sessionId: "session-1",
		agentType: "pi",
		capabilities: {},
		agentInfo: null,
	});
}

export { main };
