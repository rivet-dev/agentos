# @rivet-dev/agentos-typescript

Public AgentOS TypeScript companion package backed by AgentOS runtime primitives.

Use `@rivet-dev/agentos-typescript` when you need sandboxed TypeScript type
checking or compilation in an existing `AgentOs` VM. The package does not create
or configure a second runtime; callers choose VM packages, mounts, permissions,
and limits through `AgentOs.create(...)`, then pass that VM to
`createTypeScriptTools({ agentOs })`.

Compiler requests are streamed to a Node process over stdin. The package does
not create transport files or bootstrap directories in the VM. When `cwd` is
omitted, the compiler inherits the VM working directory; relative `cwd` values
are resolved once by the sidecar with normal Linux path semantics. Project
compilation still writes the output files requested by the project's TypeScript
configuration.
