import type {
	AgentOs,
	DurableSessionEventEntry,
	HistoryPage,
	RequestPermissionRequest,
	RequestPermissionResponse,
	SessionStreamEntry,
} from "../src/index.js";

type Equal<Left, Right> =
	(<Value>() => Value extends Left ? 1 : 2) extends <
		Value,
	>() => Value extends Right ? 1 : 2
		? true
		: false;
type Expect<Condition extends true> = Condition;
type StreamAssignable<Candidate extends SessionStreamEntry> = Candidate;
type DurableAssignable<Candidate extends DurableSessionEventEntry> = Candidate;

function checkAcpPayload(entry: DurableSessionEventEntry): void {
	switch (entry.type) {
		case "user_message_chunk":
		case "agent_message_chunk":
		case "agent_thought_chunk":
			entry.content;
			break;
		case "tool_call":
			entry.toolCallId;
			entry.title;
			break;
		case "tool_call_update":
			entry.toolCallId;
			break;
		case "plan":
			entry.entries;
			break;
		case "available_commands_update":
			entry.availableCommands;
			break;
		case "current_mode_update":
			entry.currentModeId;
			break;
		case "config_option_update":
			entry.configOptions;
			break;
		case "session_info_update":
			entry.title;
			entry.updatedAt;
			break;
		case "usage_update":
			entry.used;
			entry.size;
			break;
		case "permission_request": {
			type _Options = Expect<
				Equal<typeof entry.options, RequestPermissionRequest["options"]>
			>;
			type _ToolCall = Expect<
				Equal<typeof entry.toolCall, RequestPermissionRequest["toolCall"]>
			>;
			entry.requestId;
			break;
		}
		case "permission_response": {
			type _Outcome = Expect<
				Equal<typeof entry.outcome, RequestPermissionResponse["outcome"]>
			>;
			entry.requestId;
			entry.status;
			break;
		}
		default: {
			const exhaustive: never = entry;
			return exhaustive;
		}
	}
}

function checkStreamNarrowing(entry: SessionStreamEntry): void {
	if (entry.durability === "ephemeral") {
		type _Type = Expect<
			Equal<typeof entry.type, "agent_message_chunk" | "agent_thought_chunk">
		>;
		entry.content;
		entry.afterSequence;
		// @ts-expect-error Ephemeral entries are not members of durable history.
		entry.sequence;
		// @ts-expect-error Ephemeral entries do not have durable timestamps.
		entry.timestamp;
		return;
	}

	type _Durability = Expect<Equal<typeof entry.durability, "durable">>;
	entry.sequence;
	entry.timestamp;
	// @ts-expect-error Durable entries do not have an ephemeral cursor.
	entry.afterSequence;
	checkAcpPayload(entry);
}

type InvalidEphemeralPermission = {
	durability: "ephemeral";
	type: "permission_request";
	sessionId: string;
	afterSequence: number;
	requestId: string;
	options: RequestPermissionRequest["options"];
	toolCall: RequestPermissionRequest["toolCall"];
};

// @ts-expect-error Permission events are always durable.
type _InvalidEphemeralPermission = StreamAssignable<InvalidEphemeralPermission>;

type InvalidEphemeralPlan = {
	durability: "ephemeral";
	type: "plan";
	sessionId: string;
	afterSequence: number;
	entries: [];
};

// @ts-expect-error Only agent message and thought chunks may be ephemeral.
type _InvalidEphemeralPlan = StreamAssignable<InvalidEphemeralPlan>;

type EphemeralEntry = Extract<SessionStreamEntry, { durability: "ephemeral" }>;

// @ts-expect-error Durable history cannot contain ephemeral stream entries.
type _InvalidDurableEntry = DurableAssignable<EphemeralEntry>;

type _HistoryIsDurableOnly = Expect<
	Equal<HistoryPage["events"][number], DurableSessionEventEntry>
>;

declare const agentOs: AgentOs;

agentOs.onSessionEvent((entry) => {
	type _InferredEntry = Expect<Equal<typeof entry, SessionStreamEntry>>;
	checkStreamNarrowing(entry);
});

agentOs.onSessionEvent("session-id", (entry) => {
	type _InferredEntry = Expect<Equal<typeof entry, SessionStreamEntry>>;
	checkStreamNarrowing(entry);
});

void checkStreamNarrowing;
void checkAcpPayload;
