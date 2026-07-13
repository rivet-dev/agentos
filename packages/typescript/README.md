# @rivet-dev/agentos-typescript

Public AgentOS TypeScript companion package backed by AgentOS runtime primitives.

Use `@rivet-dev/agentos-typescript` when you need sandboxed TypeScript type
checking or compilation in an existing `AgentOs` VM. The package does not create
or configure a second runtime; callers choose VM packages, mounts, permissions,
and limits through `AgentOs.create(...)`, then pass that VM to
`createTypeScriptTools({ agentOs })`.
