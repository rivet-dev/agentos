# @rivet-dev/agentos-core

A high-level SDK for running coding agents in isolated VMs. agentOS manages the full lifecycle of virtual machines -- from filesystem setup and process management to launching AI agents via the Agent Communication Protocol (ACP).

Agents run inside isolated VMs with their own filesystem, process table, and network stack. The host only communicates through well-defined APIs, keeping agent execution fully contained.

## Features

- **VM lifecycle** — create, configure, and dispose isolated virtual machines
- **Sidecar placement** — reuse the default shared sidecar or inject an explicit sidecar handle
- **Agent sessions (ACP)** — launch coding agents (Pi, Pi CLI, OpenCode, Claude) via JSON-RPC over stdio
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
npm install @agentos-software/pi @mariozechner/pi-coding-agent
```

```typescript
import { AgentOs } from "@rivet-dev/agentos-core";

// 1. Create a VM
const vm = await AgentOs.create();

// 2. Create an agent session
const { sessionId } = await vm.createSession("pi");

// 3. Send a prompt
const response = await vm.prompt(sessionId, "Write a hello world in TypeScript");

// 4. Clean up
vm.closeSession(sessionId);
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
| `readdirRecursive` | `readdirRecursive(path: string, options?: ReaddirRecursiveOptions): Promise<DirEntry[]>` | Recursively list directory contents with metadata |
| `stat` | `stat(path: string): Promise<VirtualStat>` | Get file/directory metadata |
| `exists` | `exists(path: string): Promise<boolean>` | Check if a path exists |
| `move` | `move(from: string, to: string): Promise<void>` | Rename/move a file or directory |
| `delete` | `delete(path: string, options?: { recursive?: boolean }): Promise<void>` | Delete a file or directory |
| `mountFs` | `mountFs(path: string, driver: VirtualFileSystem, options?: { readOnly?: boolean }): void` | Mount a filesystem driver at the given path |
| `unmountFs` | `unmountFs(path: string): void` | Unmount a filesystem |

### Process Management

| Method | Signature | Description |
|--------|-----------|-------------|
| `exec` | `exec(command: string, options?: ExecOptions): Promise<ExecResult>` | Execute a shell command and wait for completion |
| `spawn` | `spawn(command: string, args: string[], options?: SpawnOptions): { pid: number }` | Spawn a long-running process |
| `listProcesses` | `listProcesses(): SpawnedProcessInfo[]` | List processes started via `spawn()` |
| `allProcesses` | `allProcesses(): ProcessInfo[]` | List all kernel processes across all runtimes |
| `processTree` | `processTree(): ProcessTreeNode[]` | Get processes organized as a parent-child tree |
| `getProcess` | `getProcess(pid: number): SpawnedProcessInfo` | Get info about a specific spawned process |
| `stopProcess` | `stopProcess(pid: number): void` | Send SIGTERM to a process |
| `killProcess` | `killProcess(pid: number): void` | Send SIGKILL to a process |

### Network

| Method | Signature | Description |
|--------|-----------|-------------|
| `fetch` | `fetch(port: number, request: Request): Promise<Response>` | Send an HTTP request to a service running inside the VM |

### Shell

| Method | Signature | Description |
|--------|-----------|-------------|
| `connectTerminal` | `connectTerminal(options?: ConnectTerminalOptions): Promise<number>` | Attach a shell directly to the host terminal and wait for exit |
| `openShell` | `openShell(options?: OpenShellOptions): { shellId: string }` | Open an interactive shell with PTY support |
| `writeShell` | `writeShell(shellId: string, data: string \| Uint8Array): void` | Write data to a shell's PTY input |
| `onShellData` | `onShellData(shellId: string, handler: (data: Uint8Array) => void): () => void` | Subscribe to shell output data |
| `resizeShell` | `resizeShell(shellId: string, cols: number, rows: number): void` | Notify terminal resize |
| `closeShell` | `closeShell(shellId: string): void` | Kill the shell process |

### Agent Sessions

| Method | Signature | Description |
|--------|-----------|-------------|
| `createSession` | `createSession(agentType: AgentType \| string, options?: CreateSessionOptions): Promise<{ sessionId: string }>` | Launch an agent and return a session ID |
| `listSessions` | `listSessions(): SessionInfo[]` | List active sessions |
| `destroySession` | `destroySession(sessionId: string): Promise<void>` | Gracefully cancel and close a session |

### Agent Registry

| Method | Signature | Description |
|--------|-----------|-------------|
| `listAgents` | `listAgents(): AgentRegistryEntry[]` | List registered agents with installation status |

### Agent Session Operations

| Method | Signature | Description |
|--------|-----------|-------------|
| `prompt` | `prompt(sessionId: string, text: string): Promise<PromptResult>` | Send a prompt and collect the agent text |
| `cancelSession` | `cancelSession(sessionId: string): Promise<JsonRpcResponse>` | Cancel ongoing agent work |
| `closeSession` | `closeSession(sessionId: string): void` | Kill the agent process and clean up |
| `onSessionEvent` | `onSessionEvent(sessionId: string, handler: SessionEventHandler): () => void` | Subscribe to session update notifications |
| `onPermissionRequest` | `onPermissionRequest(sessionId: string, handler: PermissionRequestHandler): () => void` | Subscribe to permission requests |
| `respondPermission` | `respondPermission(sessionId: string, permissionId: string, reply: PermissionReply): Promise<JsonRpcResponse>` | Reply to a permission request |
| `setSessionMode` | `setSessionMode(sessionId: string, modeId: string): Promise<JsonRpcResponse>` | Set the session mode |
| `getSessionModes` | `getSessionModes(sessionId: string): SessionModeState \| null` | Get available modes |
| `setSessionModel` | `setSessionModel(sessionId: string, model: string): Promise<JsonRpcResponse>` | Set the model |
| `setSessionThoughtLevel` | `setSessionThoughtLevel(sessionId: string, level: string): Promise<JsonRpcResponse>` | Set reasoning level |
| `getSessionConfigOptions` | `getSessionConfigOptions(sessionId: string): SessionConfigOption[]` | Get available config options |
| `rawSend` | `rawSend(sessionId: string, method: string, params?: Record<string, unknown>): Promise<JsonRpcResponse>` | Send an arbitrary ACP request |

### Exported Types

**VM & Options**
- `AgentOsOptions` — VM creation options (commandDirs, loopbackExemptPorts, mounts, additionalInstructions). Use `nodeModulesMount(...)` in `mounts` to expose a host `node_modules` tree at `/root/node_modules`.
- `AgentOsSidecarConfig` — shared-pool or explicit-handle sidecar selection for VM creation
- `AgentOsSharedSidecarOptions` — shared sidecar pool selection
- `AgentOsCreateSidecarOptions` — explicit sidecar handle creation options
- `CreateSessionOptions` — Session options (cwd, env, mcpServers, skipOsInstructions, additionalInstructions)

**Sidecar**
- `AgentOsSidecarDescription` — Sidecar identity, placement, lifecycle state, and active VM count

**Mount Configurations**
- `MountConfig` — Union of all mount types
- `MountConfigMemory` — In-memory filesystem
- `MountConfigCustom` — Caller-provided VirtualFileSystem
- `NativeMountConfig` — Declarative sidecar mount plugin configuration
- `MountConfigOverlay` — Copy-on-write overlay (lower + upper layers)
- `chunkedS3MountPlugin()` — Declarative S3-compatible native mount plugin descriptor (from `@secure-exec/core/descriptors`)

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
- `AgentType` — `"pi" | "pi-cli" | "opencode" | "claude"`
- `AgentConfig` — Agent configuration (acpAdapter, agentPackage, prepareInstructions)
- `AgentRegistryEntry` — Registry entry (id, acpAdapter, agentPackage, installed)

**Session**
- `SessionInfo` — Session summary (sessionId, agentType)
- `SessionInitData` — Data from ACP initialize response
- `SessionMode` — A mode the agent supports
- `SessionModeState` — Current mode and available modes
- `SessionConfigOption` — A configuration option the agent supports
- `AgentCapabilities` — Boolean capability flags from the agent
- `AgentInfo` — Agent identity (name, version)
- `PermissionRequest` — Permission request from an agent
- `PermissionReply` — `"once" | "always" | "reject"`
- `PermissionRequestHandler` — Handler for permission requests
- `SessionEventHandler` — Handler for live session update events

**Protocol**
- `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcNotification`, `JsonRpcError`

**Backends**
- `HostDirBackendOptions` — Options for the `createHostDirBackend()` native host-dir plugin helper
