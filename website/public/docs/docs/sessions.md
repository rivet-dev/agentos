# Sessions

Open durable ACP sessions, prompt them, read history, and restore adapters.

AgentOS sessions are durable records backed by the VM's SQLite database. The public session ID is stable across VM sleep and adapter restarts; AgentOS keeps the adapter's private ACP session ID internal. The [inspector](/docs/inspector)'s Transcript tab renders the same session stream live, with a composer for prompting sessions by hand.

## Open a session

`openSession` creates or restores a session, completes ACP negotiation, and resolves without a value. Choose and retain the `sessionId` before calling it; an omitted ID means `main`, but explicit IDs make ownership clearer. Call `getSession` separately when you need durable metadata. Repeating the same call is idempotent, but changing immutable creation options for an existing ID returns `session_conflict`.

The input supports `agent`, `cwd`, `additionalDirectories`, `env`, `mcpServers`, `permissionPolicy`, `skipOsInstructions`, and `additionalInstructions`. Omitted `cwd` defaults to `/home/agentos` in the sidecar. Actor deployments inject their SQLite UDS database automatically. Standalone core clients must configure a VM SQLite file or UDS descriptor.

## Prompt

`prompt` accepts native ACP `ContentBlock[]`, not a special AgentOS text format. It never creates a missing session. AgentOS commits the complete user message before dispatching it and never automatically replays a prompt whose delivery may have reached the adapter.

Prompt size is bounded by `limits.acp.maxPromptBytes` and `limits.acp.maxPromptBlocks`. Both are VM configuration fields; limit errors name the exact field to raise. A durable update batch must also fit the configured history byte and event budgets, and an oversized batch is rejected before it changes history.

Use an `idempotencyKey` when the caller may retry the same request. Reusing a key with different content fails. If the first call is still active, the retry waits behind that turn and receives its committed result.

## Events and history

`sessionEvent` is a flat discriminated union. Its top-level `type` is the native ACP `SessionUpdate.sessionUpdate` value, and the corresponding ACP payload fields (such as `content`, `toolCallId`, or `entries`) sit directly beside the durability envelope:

- `durability: "ephemeral"` is a live agent-message or thought delta. It is not sequenced or stored.
- `durability: "durable"` has a session sequence and is emitted only after its SQLite transaction commits. Completed/coalesced message chunks are durable.

There is no nested `update` wrapper. Permission request and response lifecycle variants use the same flat shape with top-level `options`, `toolCall`, or `outcome` fields.

`readHistory({ sessionId, before, after, limit })` reads only SQLite and never starts an adapter. `before` and `after` are exclusive and mutually exclusive. Consumers deduplicate live durable delivery by `(sessionId, sequence)`.

`getSession`, `listSessions`, `readHistory`, `getSessionConfig`, `getSessionCapabilities`, and `getSessionAgentInfo` are also SQLite-only reads. Listing uses an opaque keyset cursor; it is not a frozen snapshot of concurrent updates.

## Restoration

After VM sleep, the next `prompt` transparently starts the adapter. AgentOS prefers native ACP `session/resume`, falls back to stable `session/load`, and finally creates a fresh private ACP session with bounded continuation context from AgentOS history when the adapter does not implement either method. Adapter replay emitted during load is suppressed because SQLite is the sole AgentOS history source of truth.

The fallback transcript is bounded by `limits.acp.maxFallbackContinuationBytes`.

ACP itself does not define a portable history-reading API, and adapters implement restoration inconsistently. This is why AgentOS stores its own exact ACP updates instead of treating adapter storage as the public history database.

## Permissions

`permissionPolicy` is `reject_all`, `ask`, or `allow_all`, and defaults to `allow_all`. It controls how AgentOS answers native ACP permission requests; it does not configure VM permissions or adapter tool access. Set `permissionPolicy: "ask"` when opening the session before subscribing for interactive decisions; subscribing alone does not change the immutable policy. With `ask`, AgentOS durably records the native ACP `RequestPermissionRequest` as a `permission_request` variant in the generic session-event stream. With the default `allow_all`, AgentOS resolves the adapter request automatically and emits no permission event. Reply to an `ask` request with the exact adapter-supplied `optionId` and an explicit public session ID:

```ts
await agent.respondPermission({
  sessionId: request.sessionId,
  requestId: request.requestId,
  optionId: request.options[0].optionId,
});
```

The first valid response wins atomically. Permission requests do not expire; prompt cancellation, adapter exit, session deletion, or VM shutdown records a specific terminal reason. Accepted responses are sequenced in the same durable history. Automatic policies prefer matching one-shot options, never invent option IDs, and do not emit or persist their requests.

## Cancel, unload, and delete

- `cancelPrompt` cooperatively sends ACP cancellation. It returns `cancelled` or `no_active_prompt`.
- `unloadSession` releases the live adapter but preserves SQLite metadata and history. A later prompt restores it.
- `deleteSession` permanently removes the durable session and history. Like other session targets, an omitted ID targets `main`; repeated deletion is idempotent.

## Runtime configuration

`getSessionConfig` returns the negotiated native ACP configuration collection and a revision. `setSessionConfigOption` may restore the adapter, lets ACP validate the value, then replaces the cached collection.