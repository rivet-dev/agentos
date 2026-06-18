# agentOS

Agent OS is the agent-facing wrapper around secure-exec. It provides ACP sessions, agent adapters, quickstarts, and the public AgentOs client APIs while depending on secure-exec for the generic VM runtime.

## Boundaries

- Local Agent OS development dependencies on secure-exec must point to `../secure-exec`.
- Keep generic runtime, kernel, VFS, language execution, and registry software behavior in secure-exec.
- Agent OS owns ACP, sessions, agent adapters, toolkit semantics, quickstarts, and the AgentOs facade.
- Call OS instances VMs, never sandboxes.
- The protocol has no backwards compatibility. Clients and the sidecar ship in same-version lockstep, so never add protocol or config versioning, runtime negotiation, fallbacks, or converters. Configs such as `CreateVmConfig` carry no `version` field; the single same-version wire handshake is the only version check. Change the protocol freely and update both sides together.

## Agent Sessions

- Every public method on `packages/core/src/agent-os.ts` must stay mirrored by RivetKit actor actions after the user confirms the Rivet repo path.
- Subscription methods are delivered through actor events; lifecycle behavior belongs in actor sleep/destroy hooks.
- Agent adapters must use real upstream agent SDKs. Do not replace SDK adapters with direct API-call stubs.
- Host-native agent wrappers are not allowed; agents run through the VM runtime supplied by secure-exec.

## Extension Authoring

- Agent OS extension payloads use the secure-exec `Ext` envelope with Agent OS-owned namespaces and generated ACP payloads.
- Keep ACP decoding and session state in Agent OS wrapper code, not in secure-exec core sidecar code.
- The agent-os sidecar wrapper embeds and extends secure-exec; secure-exec must remain free of ACP, agent, and session dependencies.

## Website And Docs

- The Agent OS website and docs live in `website/` (Astro + Starlight) and deploy to `agentos-sdk.dev` (docs at `agentos-sdk.dev/docs`). The marketing pages and docs were migrated out of `rivet.dev/agent-os` and `rivet.dev/docs/agent-os`, which now 301-redirect to this domain.
- The core quickstart under `examples/quickstart/` and the RivetKit example must stay behaviorally identical.
- Every quickstart change needs a matching automated test in the same change.
- Confirm the docs repo path with the user before editing Agent OS docs.
- Keep `website/src/data/registry.ts` current when package names or registry entries change.
