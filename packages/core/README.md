# AgentOS runtime internals

`@rivet-dev/agentos-core` is the shared runtime implementation used by the
public AgentOS packages. It is published only as a transitive dependency and is
not a supported user-facing install target.

Install the package for your use case instead:

```sh
# Actors, sessions, and the complete AgentOS SDK
pnpm add @rivet-dev/agentos

# AgentOS language execution for JavaScript and TypeScript
pnpm add @rivet-dev/agentos-javascript

# AgentOS language execution for Python
pnpm add @rivet-dev/agentos-python
```

See the [AgentOS documentation](https://agentos-sdk.dev/docs) for supported
APIs.
