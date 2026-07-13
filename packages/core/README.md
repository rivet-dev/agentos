# @rivet-dev/agentos-core

A high-level SDK for running coding agents in isolated VMs. agentOS manages the full lifecycle of virtual machines -- from filesystem setup and process management to launching AI agents via the Agent Communication Protocol (ACP).

Agents run inside isolated VMs with their own filesystem, process table, and network stack. The host only communicates through well-defined APIs, keeping agent execution fully contained.

## Features

- **VM lifecycle** — create, configure, and dispose isolated virtual machines
- **Sidecar placement** — reuse the default shared sidecar or inject an explicit sidecar handle
- **Agent sessions (ACP)** — launch coding agents (Pi, OpenCode, Claude) via JSON-RPC over stdio
- **Filesystem operations** — read, write, mkdir, stat, move, delete, recursive listing, batch read/write
- **Process management** — spawn, exec, stop, kill processes; inspect process trees across all runtimes
- **Agent registry** — discover available agents and their installation status
- **Networking** — reach services running inside the VM via `fetch()`
- **Shell access** — open interactive shells with PTY support
- **Mount backends** — memory, native host directory mounts, S3, overlay (copy-on-write), or custom VirtualFileSystem

## Quick Start

```bash
npm install @rivet-dev/agentos-core
# Install an agent adapter + its underlying agent
npm install @agentos-software/pi
```

```typescript
import { AgentOs } from "@rivet-dev/agentos-core";

// 1. Create a VM
const vm = await AgentOs.create();

// 2. Create the default agent session
await vm.openSession({ agent: "pi" });

// 3. Send a prompt
const response = await vm.prompt({
  content: [{ type: "text", text: "Write a hello world in TypeScript" }],
});

// 4. Clean up
await vm.deleteSession();
await vm.dispose();
```

## API Reference

### Lifecycle

| Method | Signature | Description |
|--------|-----------|-------------|
| `create` | `static create(options?: AgentOsOptions): Promise<AgentOs>` | Create and boot a new VM |
| `getSharedSidecar` | `static getSharedSidecar(options?: AgentOsSharedSidecarOptions): Promise<AgentOsSidecar>` | Get or create a shared sidecar handle for a pool |
| `createSidecar` | `static createSidecar(options?: AgentOsCreateSidecarOptions): Promise<AgentOsSidecar>` | Create an explicit sidecar handle |
| `dispose` | `dispose(): Promise<void>` | Shut down the VM and all sessions |

### Sidecars

| Surface | Signature | Description |
|--------|-----------|-------------|
| `sidecar` | `AgentOsSidecar` | Sidecar handle backing the VM |
| `describe` | `sidecar.describe(): AgentOsSidecarDescription` | Inspect sidecar placement, state, and active VM count |
| `dispose` | `sidecar.dispose(): Promise<void>` | Dispose the sidecar handle and any active VMs leased from it |

### Filesystem

| Method | Signature | Description |
|--------|-----------|-------------|
| `readFile` | `readFile(path: string): Promise<Uint8Array>` | Read a file |
| `writeFile` | `writeFile(path: string, content: string \| Uint8Array): Promise<void>` | Write a file |
| `readFiles` | `readFiles(paths: string[]): Promise<BatchReadResult[]>` | Batch read multiple files |
| `writeFiles` | `writeFiles(entries: BatchWriteEntry[]): Promise<BatchWriteResult[]>` | Batch write multiple files (creates parent dirs) |
| `mkdir` | `mkdir(path: string): Promise<void>` | Create a directory |
| `readdir` | `readdir(path: string): Promise<string[]>` | List directory entries |
| `readdirEntries` | `readdirEntries(path: string): Promise<ReaddirEntry[]>` | List immediate directory entries and types in one operation |
| `readdirRecursive` | `readdirRecursive(path: string, options?: ReaddirRecursiveOptions): Promise<DirEntry[]>` | Recursively list directory contents with metadata |
| `stat` | `stat(path: string): Promise<VirtualStat>` | Get file/directory metadata |
| `exists` | `exists(path: string): Promise<boolean>` | Check if a path exists |
| `move` | `move(from: string, to: string): Promise<void>` | Rename/move a file or directory |
| `remove` | `remove(path: string, options?: { recursive?: boolean }): Promise<void>` | Remove a file or directory |
| `mountFs` | `mountFs(descriptor: DynamicMountDescriptor): Promise<void>` | Mount a sidecar-owned filesystem descriptor |
| `unmountFs` | `unmountFs(path: string): Promise<void>` | Unmount a filesystem |
| `listMounts` | `listMounts(): Promise<MountInfo[]>` | Read sanitized live mount metadata from the sidecar |
| `exportRootFilesystem` | `exportRootFilesystem({ maxBytes }): Promise<RootSnapshotExport>` | Export a bounded root-filesystem snapshot |

### Process Management

| Method | Signature | Description |
|--------|-----------|-------------|
| `exec` | `exec(command: string, options?: ExecOptions): Promise<ExecResult>` | Execute a shell command and wait for completion |
| `spawn` | `spawn(command: string, args: string[], options?: SpawnOptions): { pid: number }` | Spawn a long-running process |
| `onProcessOutput` | `onProcessOutput(pid, handler): () => void` | Subscribe to unified stdout/stderr DTOs |
| `onProcessExit` | `onProcessExit(pid, handler): () => void` | Subscribe to process exit DTOs |
| `listProcesses` | `listProcesses(): SpawnedProcessInfo[]` | List processes started via `spawn()` |
| `allProcesses` | `allProcesses(): ProcessInfo[]` | List all kernel processes across all runtimes |
| `processTree` | `processTree(): ProcessTreeNode[]` | Get processes organized as a parent-child tree |
| `getProcess` | `getProcess(pid: number): SpawnedProcessInfo` | Get info about a specific spawned process |
| `stopProcess` | `stopProcess(pid: number): void` | Send SIGTERM to a process |
| `killProcess` | `killProcess(pid: number): void` | Send SIGKILL to a process |

### Network

| Method | Signature | Description |
|--------|-----------|-------------|
| `httpRequest` | `httpRequest(request: HttpRequest): Promise<HttpResponse>` | Send a buffered HTTP request to a service running inside the VM |

### Shell

| Method | Signature | Description |
|--------|-----------|-------------|
| `connectTerminal` | `connectTerminal(options?: ConnectTerminalOptions): Promise<number>` | Attach a shell directly to the host terminal and wait for exit |
| `openShell` | `openShell(options?: OpenShellOptions): { shellId: string }` | Open an interactive shell with PTY support |
| `writeShell` | `writeShell(shellId: string, data: string \| Uint8Array): void` | Write data to a shell's PTY input |
| `onShellData` | `onShellData(shellId: string, handler: (data: Uint8Array) => void): () => void` | Subscribe to ordered PTY output (stdout and stderr exactly once) |
| `resizeShell` | `resizeShell(shellId: string, cols: number, rows: number): void` | Notify terminal resize |
| `closeShell` | `closeShell(shellId: string): void` | Kill the shell process |

### Agent Sessions

| Method | Signature | Description |
|--------|-----------|-------------|
| `openSession` | `openSession(input: OpenSessionInput): Promise<void>` | Create or restore a durable session with a caller-chosen ID |
| `getSession` | `getSession(input?: SessionTarget): Promise<SessionInfo>` | Read SQLite metadata without starting an adapter |
| `listSessions` | `listSessions(input?: ListSessionsInput): Promise<SessionPage>` | Page through SQLite metadata without starting adapters |
| `unloadSession` | `unloadSession(input?: SessionTarget): Promise<void>` | Stop the adapter while retaining history |
| `deleteSession` | `deleteSession(input?: SessionTarget): Promise<void>` | Stop the adapter and delete durable state; omitted ID targets main |

### Agent Registry

| Method | Signature | Description |
|--------|-----------|-------------|
| `listAgents` | `listAgents(): AgentRegistryEntry[]` | List registered agents with installation status |

### Agent Session Operations

| Method | Signature | Description |
|--------|-----------|-------------|
| `prompt` | `prompt(input: PromptInput): Promise<PromptResult>` | Send native ACP content blocks and durably commit the turn |
| `cancelPrompt` | `cancelPrompt(input?: SessionTarget): Promise<CancelPromptResult>` | Cancel active agent work |
| `onSessionEvent` | `onSessionEvent(sessionId: string, handler: SessionEventHandler): () => void` | Subscribe to native ACP update and permission request/response variants |
| `respondPermission` | `respondPermission(input: PermissionResponse): Promise<PermissionResponseResult>` | Select an exact adapter-supplied ACP permission option |
| `getSessionConfig` | `getSessionConfig(input?: SessionTarget): Promise<SessionConfig>` | Read cached native ACP configuration |
| `setSessionConfigOption` | `setSessionConfigOption(input: SetSessionConfigOptionInput): Promise<SessionConfig>` | Set a native ACP string or boolean option |
| `readHistory` | `readHistory(input?: ReadHistoryInput): Promise<HistoryPage>` | Read authoritative durable ACP updates and permission events from SQLite |

### Exported Types

**VM & Options**
- `AgentOsOptions` — VM creation options (commandDirs, loopbackExemptPorts, mounts). Use `nodeModulesMount(...)` in `mounts` to expose a host `node_modules` tree at `/root/node_modules`.
- `AgentOsSidecarConfig` — shared-pool or explicit-handle sidecar selection for VM creation
- `AgentOsSharedSidecarOptions` — shared sidecar pool selection
- `AgentOsCreateSidecarOptions` — explicit sidecar handle creation options
- `OpenSessionInput` — Durable session identity and creation options (agent, cwd, env, mcpServers, skipOsInstructions, additionalInstructions)

**Sidecar**
- `AgentOsSidecarDescription` — Sidecar identity, placement, lifecycle state, and active VM count

**Mount Configurations**
- `MountConfig` — Union of all mount types
- `MountConfigMemory` — In-memory filesystem
- `MountConfigCustom` — Caller-provided VirtualFileSystem
- `NativeMountConfig` — Declarative sidecar mount plugin configuration
- `MountConfigOverlay` — Copy-on-write overlay (lower + upper layers)
- `chunkedS3MountPlugin()` — Declarative S3-compatible native mount plugin descriptor (from `@rivet-dev/agentos-runtime-core/descriptors`)

**MCP Servers**
- `McpServerConfig` — Union of local and remote MCP configs
- `McpServerConfigLocal` — Local MCP server (command, args, env)
- `McpServerConfigRemote` — Remote MCP server (url, headers)

**Process**
- `ProcessInfo` — Kernel process info (pid, ppid, pgid, sid, driver, command, args, cwd, status, exitCode, startTime, exitTime)
- `SpawnedProcessInfo` — Info for processes created via `spawn()` (pid, command, args, running, exitCode)
- `ProcessTreeNode` — ProcessInfo with `children: ProcessTreeNode[]`

**Filesystem**
- `DirEntry` — Directory entry (path, type, size)
- `ReaddirRecursiveOptions` — Options for recursive listing (maxDepth, exclude)
- `BatchWriteEntry` — Entry for batch writes (path, content)
- `BatchWriteResult` — Result of a batch write (path, success, error?)
- `BatchReadResult` — Result of a batch read (path, content, error?)

**Agent**
- `AgentType` — `string` (a package manifest `name`, e.g. `"pi"`, `"claude"`); agents are resolved dynamically from the configured `/opt/agentos` package manifests, so any manifest `name` is valid
- `AgentConfig` — Agent configuration (adapterEntrypoint, launchArgs, defaultEnv)
- `AgentRegistryEntry` — Registry entry (id, acpAdapter, agentPackage, installed)

**Session**
- `SessionInfo` — Durable session summary and current state
- `SessionStreamEntry` — Generic live union of durable session updates, permission requests/responses, and ephemeral message deltas
- `DurableSessionEventEntry` — Durable history/event union keyed by session sequence
- `HistoryPage` — Cursor-based durable event page returned by `readHistory()`
- `SessionConfigOption` — A configuration option the agent supports
- `SessionCapabilities` — Native ACP capabilities cached for a durable session
- `SessionAgentInfo` — Native ACP adapter identity
- `PermissionPolicy` — Sidecar-owned `"allow_all" | "reject_all" | "ask"` strategy
- `PermissionResponse` / `PermissionResponseResult` — Explicit-session native ACP option selection and its accepted/terminal result
- `SessionEventHandler` — Handler for the generic live session-event union

**Protocol**
- `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification`, `JsonRpcError`

**Backends**
- `HostDirBackendOptions` — Options for the `createHostDirBackend()` native host-dir plugin helper
