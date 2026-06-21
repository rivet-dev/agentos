# agentOS Packages

- Client packages must stay same-version with the sidecar: assert the single protocol version integer, and do not add wire back-compat, runtime negotiation, or converters.
- Generated client layers return raw generated protocol types; the `AgentOs` facade in `@rivet-dev/agentos-core` is the only sanctioned ergonomic wrapper.
- Generic secure-exec clients must stay agent-agnostic and must not branch on the Agent OS ACP namespace.
- secure-exec packages must never depend on agent-os packages; dependency direction is strictly agent-os to secure-exec and must be CI-enforced after the split.
- The sidecar remains the source of truth for runtime behavior; TypeScript package code should forward generated requests instead of reimplementing sidecar state machines.
- Cron and agent configuration types are Rust-owned after the split; TypeScript packages may re-export or mirror them only in lockstep.
