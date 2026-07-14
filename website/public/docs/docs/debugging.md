# Debugging

Capture agent logs and runtime (sidecar) logs to diagnose sessions, tool calls, and crashes.

Two log streams help diagnose what's happening inside a VM: the **agent's** own output and the **runtime (sidecar)** logs.

## Agent logs (`onAgentStderr`)

The coding agent (ACP adapter) runs as a process inside the VM and uses **stdout for the ACP protocol**, so its **stderr** carries the agent's logs, warnings, and crash output — the first place to look when a tool call or session fails mid-turn. Capture it with `onAgentStderr` on the VM:

It's a VM-level option covering every session's agent process; if omitted, chunks are written to the host `process.stderr` by default. See [Sessions → Agent logs](/docs/sessions#agent-logs).

## Agent crashes (`onAgentExit`)

If the agent process exits without `closeSession()`, the runtime logs the exit, **auto-restarts the agent** (bounded to 3 restarts per session, re-attaching the same session id when the agent supports native resume), and fires `onAgentExit` with the outcome:

```ts
const agentOs = await AgentOs.create({
  software: [pi],
  onAgentExit(event) {
    // event: { sessionId, agentType, processId, pid, exitCode,
    //          restart: "restarted" | "unsupported" | "failed" | "exhausted",
    //          restartCount, maxRestarts }
    console.warn(`agent exited (code ${event.exitCode}), restart=${event.restart}`);
  },
});
```

Only `restart === "restarted"` leaves the session usable; every other outcome means the session was evicted. The crash *reason* is on the agent's stderr (above); the exit event tells you it died and whether it recovered. See [Sessions → Agent crashes and auto-restart](/docs/sessions#agent-crashes-and-auto-restart).

## Runtime logs (sidecar)

The agentOS sidecar emits structured **logfmt** logs for request handling, networking, and lifecycle. Configure them with environment variables on the **host process** (the sidecar inherits the host environment):

| env var | effect |
|---------|--------|
| `AGENTOS_LOG_LEVEL` / `LOG_LEVEL` / `RUST_LOG` | log filter, in that priority. Uses [EnvFilter](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax, e.g. `debug`, `info`, `agentos_sidecar=debug,info`. Default `info`. |
| `RUST_LOG_FORMAT` | `logfmt` (default) or `text` |
| `AGENTOS_LOG_FILE` | append logs to this file instead of stderr (never stdout, which carries the wire protocol) |
| `AGENTOS_MAX_SESSIONS_PER_CONNECTION` | maximum open wire sessions on one sidecar connection; defaults to `1024`. Raise this only for hosts that intentionally keep more concurrent VMs/sessions. |
| `RUST_LOG_{SPAN_NAME,SPAN_PATH,TARGET,LOCATION,MODULE_PATH,ANSI_COLOR}` | per-field toggles (`=1` to enable) |

```bash
AGENTOS_LOG_LEVEL=debug AGENTOS_LOG_FILE=./sidecar.log RUST_LOG_FORMAT=logfmt node app.mjs
```

Produces logfmt lines such as:

```text
ts=2026-… level=info  message="ext request received" kind=create_session
ts=2026-… level=info  message="ext request handled"  kind=create_session elapsed_ms=1798
ts=2026-… level=debug message="querying: api.anthropic.com. A"
```

Most sidecar log activity is on the session/ACP path. A bare `AgentOs.create()` or a single `exec()` emits almost nothing — create a session (and send a prompt) to see request-handling logs.

Use **agent logs** to see what the agent did (tool calls, model errors), and **runtime logs** to see what the sidecar did around it (request timing, DNS, lifecycle).
