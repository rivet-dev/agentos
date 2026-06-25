# Persistence & Sleep

How agentOS persists data and manages sleep/wake cycles.

agentOS automatically persists the `/home/agentos` filesystem and session transcripts (with sequence numbers for replay) across sleep/wake, sleeping after a configurable grace period (15 minutes by default) and waking automatically when a client connects or a cron job triggers.

## What persists across sleep

| Data | Storage | Persists? |
|------|---------|-----------|
| Files in `/home/agentos` | Persistent filesystem | Yes |
| Session records | SQLite (`agent_os_sessions`) | Yes |
| Session event history | SQLite (`agent_os_session_events`) | Yes |
| Preview URL tokens | SQLite (`agent_os_preview_tokens`) | Yes |
| Cron job definitions | Actor state | Yes |
| Running processes | VM kernel | No |
| Active shells | VM kernel | No |
| In-memory mounts | VM memory | No |
| VM kernel state | VM memory | No |

## What prevents sleep

The actor stays awake as long as any of these are active:

- **Active sessions** (created but not closed/destroyed)
- **Running processes** (spawned but not exited)
- **Active shells** (opened but not closed)
- **Pending hooks** (server-side callbacks still executing)

When all activity stops, the sleep grace period begins.

## Sleep grace period

After all activity stops, the actor waits 15 minutes before sleeping. This allows for brief pauses between interactions without restarting the VM.

```
Activity stops ──> 15 min grace period ──> Actor sleeps
                                           (VM shutdown, processes killed)

New client connects ──> Actor wakes ──> VM boots ──> Filesystem restored
```

## Timeouts

| Setting | Default | Description |
|---------|---------|-------------|
| Action timeout | 15 minutes | Maximum time for any single action |
| Sleep grace period | 15 minutes | Time before sleeping after all activity stops |

These are set internally by the `agentOS()` factory and cannot be overridden per-call.

## Sleep vs destroy

| | Sleep | Destroy |
|-|-------|---------|
| Filesystem | Preserved | Deleted |
| Session records | Preserved | Deleted |
| Event history | Preserved | Deleted |
| Preview tokens | Preserved | Deleted |
| VM state | Lost | Lost |
| Processes | Killed | Killed |

## VM boot and shutdown events

Subscribe to `vmBooted` and `vmShutdown` events to track VM lifecycle.

## Resuming after sleep

When the actor wakes up, the VM boots and the filesystem is restored from SQLite, session records and event history are immediately available, and processes and shells from the previous session are gone. Clients can reconnect, list prior work with `listPersistedSessions` (which works without a running VM), and replay a session's persisted transcript with `getSessionEvents`.

## Persisted tables schema

### `agent_os_fs_entries`

Stores the virtual filesystem.

| Column | Type | Description |
|--------|------|-------------|
| `path` | TEXT PRIMARY KEY | File or directory path |
| `is_directory` | INTEGER | 1 for directory, 0 for file |
| `content` | BLOB | File content |
| `mode` | INTEGER | POSIX mode bits |
| `size` | INTEGER | File size in bytes |
| `atime_ms` | INTEGER | Access time (ms) |
| `mtime_ms` | INTEGER | Modification time (ms) |
| `ctime_ms` | INTEGER | Change time (ms) |
| `birthtime_ms` | INTEGER | Birth time (ms) |

### `agent_os_sessions`

Stores session metadata.

| Column | Type | Description |
|--------|------|-------------|
| `session_id` | TEXT PRIMARY KEY | Unique session identifier |
| `agent_type` | TEXT | Agent type (e.g. "pi") |
| `capabilities` | TEXT (JSON) | Agent capabilities |
| `agent_info` | TEXT (JSON) | Agent metadata |
| `created_at` | INTEGER | Creation timestamp (ms) |

### `agent_os_session_events`

Stores session event history.

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER PRIMARY KEY | Auto-incrementing ID |
| `session_id` | TEXT | Session reference |
| `seq` | INTEGER | Sequence number within session |
| `event` | TEXT (JSON) | JSON-RPC notification |
| `created_at` | INTEGER | Timestamp (ms) |