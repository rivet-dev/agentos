---
title: "Persistence & Sleep"
description: "How agentOS persists files and manages sleep/wake cycles."
---

agentOS persists the `/home/agentos` filesystem across actor sleep and wakes the VM automatically when a client calls it. Live sessions, processes, shells, and session events do not survive VM shutdown.

## What persists across sleep

| Data | Storage | Persists? |
|------|---------|-----------|
| Files in `/home/agentos` | Actor SQLite over UDS | Yes |
| Preview URL tokens | Actor SQLite | Yes |
| Active agent sessions | VM memory | No |
| Session event history | Live event stream | No |
| Cron job definitions | VM memory | No |
| Running processes | VM kernel | No |
| Active shells | VM kernel | No |
| In-memory mounts | VM memory | No |

The native sidecar reads and writes filesystem chunks directly through the actor's authenticated SQLite Unix socket. File contents do not pass through the TypeScript or JavaScript actor layer.

## What prevents sleep

The actor stays awake while sessions, processes, shells, or server-side hooks are active. Once activity stops, the configured sleep grace period begins.

```text
Activity stops -> grace period -> actor sleeps and the VM shuts down

New call -> actor wakes -> VM boots -> filesystem restored from actor SQLite
```

The default action timeout and sleep grace period are both 15 minutes. They can be tightened through the actor's `options` configuration.

## Sleep vs destroy

| | Sleep | Destroy |
|-|-------|---------|
| Filesystem | Preserved | Deleted |
| Preview tokens | Preserved | Deleted |
| Live sessions and events | Lost | Lost |
| Processes and shells | Lost | Lost |

## VM lifecycle events

Subscribe to `vmBooted` and `vmShutdown` to observe VM lifecycle changes.

## Reading files after sleep

When the actor wakes, a fresh VM is created and its durable filesystem is restored. Create a new session and read or continue work from the files under `/home/agentos`; do not expect the previous session or missed event stream to return.

## SQLite tables

`agentos_vfs_metadata_heads` and `agentos_vfs_metadata_chunks` store the chunked inode and directory metadata for each filesystem namespace. `agentos_vfs_blocks` stores content-addressed file chunks. The sidecar owns these schemas and accesses them directly over UDS.

`agent_os_preview_tokens` is actor-owned metadata used for signed preview URLs. Agent session records and transcripts are intentionally not persisted by the actor.
