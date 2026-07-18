import type {
	ContentBlock,
	Implementation,
	McpServer,
	RequestPermissionRequest,
	RequestPermissionResponse,
	SessionConfigOption,
	SessionUpdate,
	StopReason,
} from "@agentclientprotocol/sdk";

export type {
	ContentBlock,
	McpServer,
	SessionConfigOption,
	SessionUpdate,
	StopReason,
};
export type { RequestPermissionRequest, RequestPermissionResponse };

export type JsonValue =
	| null
	| boolean
	| number
	| string
	| JsonValue[]
	| { [key: string]: JsonValue };

export interface SerializedError {
	code: string;
	message: string;
	details?: JsonValue;
	retryable?: boolean;
}

/** Exact upstream ACP MCP server configuration. */
export type McpServerConfig = McpServer;

export type PermissionPolicy = "reject_all" | "ask" | "allow_all";

export interface OpenSessionInput {
	sessionId?: string;
	agent: string;
	cwd?: string;
	additionalDirectories?: string[];
	env?: Record<string, string>;
	mcpServers?: McpServerConfig[];
	/**
	 * Immutable AgentOS strategy for native ACP permission requests. Defaults to
	 * `allow_all`. This does not configure VM permissions or adapter tool access.
	 */
	permissionPolicy?: PermissionPolicy;
	skipOsInstructions?: boolean;
	additionalInstructions?: string;
}

export interface SessionTarget {
	sessionId?: string;
}

export type DeleteSessionInput = SessionTarget;

export type SessionState =
	| { status: "idle" }
	| { status: "running"; startedAt: string }
	| {
			status: "waiting";
			waitingSince: string;
			requests: PendingPermissionRequest[];
	  }
	| { status: "failed"; error: SerializedError };

export interface SessionInfo {
	sessionId: string;
	agent: string;
	cwd: string;
	additionalDirectories: string[];
	state: SessionState;
	latestSequence: number;
	title?: string;
	metadata?: Record<string, JsonValue> | null;
	createdAt: string;
	updatedAt: string;
}

export interface ListSessionsInput {
	cursor?: string;
	limit?: number;
}

export interface SessionPage {
	sessions: SessionInfo[];
	nextCursor: string | null;
}

export interface PromptInput {
	sessionId?: string;
	idempotencyKey?: string;
	content: ContentBlock[];
}

export interface UserMessage {
	id: string;
	role: "user";
	content: ContentBlock[];
}

export interface AgentMessage {
	id: string;
	role: "agent";
	content: ContentBlock[];
}

export interface PromptResult {
	sessionId: string;
	message: AgentMessage | null;
	stopReason: StopReason;
}

export interface CancelPromptResult {
	status: "cancelled" | "no_active_prompt";
}

export interface DurableSessionEventEnvelope {
	durability: "durable";
	sessionId: string;
	sequence: number;
	timestamp: string;
}

export interface EphemeralSessionEventEnvelope {
	durability: "ephemeral";
	sessionId: string;
	afterSequence: number;
}

type FlatAcpSessionUpdate<Update extends SessionUpdate = SessionUpdate> =
	Update extends SessionUpdate
		? Omit<Update, "sessionUpdate"> & { type: Update["sessionUpdate"] }
		: never;

/** Exact ACP session-update payloads with the ACP discriminator promoted to `type`. */
export type AcpSessionEvent = FlatAcpSessionUpdate;

export type DurableSessionUpdateEntry = DurableSessionEventEnvelope &
	AcpSessionEvent;

export type DurablePermissionRequestEntry = DurableSessionEventEnvelope & {
	type: "permission_request";
	requestId: string;
} & Omit<RequestPermissionRequest, "sessionId">;

export type DurablePermissionResponseEntry = DurableSessionEventEnvelope & {
	type: "permission_response";
	requestId: string;
	status: "accepted" | "not_pending";
	reason?: PermissionTerminalReason;
} & RequestPermissionResponse;

export type DurableSessionEventEntry =
	| DurableSessionUpdateEntry
	| DurablePermissionRequestEntry
	| DurablePermissionResponseEntry;

export type EphemeralSessionEventEntry = EphemeralSessionEventEnvelope &
	FlatAcpSessionUpdate<
		Extract<
			SessionUpdate,
			{
				sessionUpdate: "agent_message_chunk" | "agent_thought_chunk";
			}
		>
	>;

export type SessionStreamEntry =
	| DurableSessionEventEntry
	| EphemeralSessionEventEntry;

export interface ReadHistoryInput extends SessionTarget {
	before?: number;
	after?: number;
	limit?: number;
}

export interface HistoryPage {
	events: DurableSessionEventEntry[];
	hasMoreBefore: boolean;
	hasMoreAfter: boolean;
}

export interface SessionConfig {
	revision: number;
	options: SessionConfigOption[];
}

export interface SetSessionConfigOptionInput extends SessionTarget {
	configId: string;
	value: string | boolean;
}

export interface SessionCapabilities {
	protocolVersion: number;
	loadSession: boolean;
	prompt?: { audio?: boolean; embeddedContext?: boolean; image?: boolean };
	mcp?: { http?: boolean; sse?: boolean };
	session?: {
		list?: boolean;
		resume?: boolean;
		close?: boolean;
		delete?: boolean;
		additionalDirectories?: boolean;
	};
	extensions?: Record<string, JsonValue>;
}

export type SessionAgentInfo = Implementation;

export interface PendingPermissionRequest {
	requestId: string;
	options: RequestPermissionRequest["options"];
	toolCall: RequestPermissionRequest["toolCall"];
	_meta?: RequestPermissionRequest["_meta"];
}

export interface PermissionResponse {
	sessionId: string;
	requestId: string;
	optionId: string;
}

export type PermissionTerminalReason =
	| "already_resolved"
	| "prompt_cancelled"
	| "adapter_exited"
	| "session_deleted"
	| "vm_shutdown"
	| "request_not_found";

export type PermissionResponseResult =
	| { status: "accepted" }
	| { status: "not_pending"; reason: PermissionTerminalReason };
