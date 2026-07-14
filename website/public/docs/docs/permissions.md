# Permissions

The per-scope kernel permission policy that gates every guest syscall in the sandbox.

The sandbox permission policy is the kernel-level enforcement layer. Every guest syscall the agent's sandboxed code makes is checked against a per-scope policy before any host resource is touched.

- **Six scopes**, configured independently: `fs`, `network`, `childProcess`, `process`, `env`, `binding`.
- **Each scope** is a mode (`"allow"` or `"deny"`), or a rule set.
- **A denied operation** is rejected with `EACCES` before any host resource is touched.
- **Omitted top-level scopes inherit the sidecar allow-all default**, so partial policies remain explicit overrides.

For the higher-level agent tool-approval layer (human-in-the-loop, auto-approve), see [Approvals](/docs/approvals).

## Defaults and merge semantics

Omitting `permissions` selects the sidecar-owned allow-all product default. All six scopes—`fs`, `network`, `childProcess`, `process`, `env`, and `binding`—are `allow`.

An explicit partial top-level policy changes only the scopes it names. Omitted top-level scopes inherit `allow`. Inside an explicit rule-set scope, an omitted `default` means `deny`; set `default: "allow"` for a deny-list or `default: "deny"` for an allow-list.

Allow-all authorizes only capabilities present inside or explicitly configured for the VM. It does not expose the host filesystem, host process table, or unrestricted host sockets.

## Permission scopes

| Scope | Controls | Default |
|---|---|---|
| `fs` | Filesystem reads, writes, and metadata operations | `allow` |
| `network` | Outbound connections: `fetch`, HTTP, DNS, and inbound `listen` | `allow` |
| `childProcess` | Spawning child processes | `allow` |
| `process` | Process-control operations | `allow` |
| `env` | Environment variable access | `allow` |
| `binding` | Invoking bindings registered with the runtime | `allow` |

## Bind a policy to the VM

A policy is a plain object keyed by scope. Pass it as `permissions` to `agentOS(...)` and it gates every guest syscall on that VM.

## Grant or deny a whole scope

The simplest value for a scope is a single mode string. `"allow"` permits every operation in the scope; `"deny"` rejects every one with `EACCES`. Omitted top-level scopes inherit `allow`, so list every scope that must be restricted.

```ts
const permissions = {
	network: "deny",  // turn off network egress
	fs: "deny",       // turn off all filesystem access
};
```

There is no typed `"ask"` mode. Interactive, human-in-the-loop approval lives in the higher-level [Approvals](/docs/approvals) layer, not the kernel policy. To block at the kernel level, use `"deny"`.

## Allow only specific filesystem paths

For finer control, a scope can be a rule set instead of a bare mode: a `default` mode plus an ordered list of `rules`. The `fs` scope matches by `paths` (filesystem globs). Each rule names its `operations` (`read`, `write`, `stat`, `readdir`, `create_dir`, `rm`, `rename`, `symlink`, `readlink`, `chmod`, `truncate`, `mount_sensitive`, or `["*"]` for all). Last matching rule wins; if no rule matches, `default` applies.

To invert it, flip `default` to `"deny"` and allow just one subtree:

## Allow only specific network hosts

Every non-`fs` scope matches by `patterns` instead of `paths`. For `network`, a pattern is a host (or `host:port`), and the operations are `fetch`, `http`, `dns`, and `listen`.

## Allow only specific bindings

Bindings registered with the runtime are gated by the `binding` scope, matched by name via `patterns`. Bindings have no sub-operations, so pass `["*"]` for `operations`.

The `childProcess`, `process`, and `env` scopes work the same way: `childProcess` patterns match the command (`operations: ["spawn"]`), `env` patterns match the variable name (`operations: ["read", "write"]`), and `process` is matched by pattern with `operations: ["*"]`.

## Combine policies and see denials

Each policy above sets one scope, so you can spread several into one `permissions` object and bind them together.

When a scope or matching rule denies an operation, the kernel rejects it with `EACCES` before any host resource is touched. For example, with `network: "deny"`, an outbound `fetch()` inside the guest throws:

```
EACCES: permission denied, tcp://example.com:80: blocked by network.http policy
```