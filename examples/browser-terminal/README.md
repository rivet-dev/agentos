---
title: "Browser Terminal"
description: "Two full xterm.js Agent OS terminals: one running the VM locally in the browser, and one driving a durable VM through the agentOS() RivetKit actor API."
category: "Processes & Shell"
order: 2
---

This example contains two deliberately separate versions of the same xterm + PTY
demo:

- **In-browser VM** (`/browser.html`) — the Agent OS WASM sidecar, kernel, VFS,
  shell commands, Pi CLI, and PTYs all execute inside the browser tab. It does not
  call the Actor API. Each terminal tab is an isolated in-memory browser VM.
- **Actor API** (`/actor.html`) — the React client talks to the shipped Agent OS
  actor (`agentOS()` from `@rivet-dev/agentos`) over its live
  [RivetKit](https://rivetkit.org) connection. The VM and PTYs execute behind the
  actor and survive browser reconnects.

The root page (`/`) is a mode selector and both terminal pages are visibly labeled
with their execution boundary.

## Actor API version

- **Left sidebar** — a list of VMs. Each is one Agent OS VM (one RivetKit actor
  instance). The VM ids are kept in `localStorage`, so reopening the page — or
  clicking a VM again — reconnects to the same running VM.
- **Tabs** — each VM can have multiple terminal sessions (PTY shells).
- **Reconnect** — the actor keeps its VM (and shells) alive, so a browser that
  reconnects re-adopts the running shells (by the ids it saved in `localStorage`)
  and resumes their live I/O.

### How it works

```
Browser (React + xterm.js)                Node (server.ts)
  ├─ useActor({ name:"shellVm", key })      ├─ agentOS({ software:[…] })
  ├─ openShell / writeShell / resize ──────▶│    setup({ use:{ shellVm } })
  ├─ closeShell                             │    registry.start()
  └─ conn.on("shellData"|"shellExit") ◀────┘  openShell ─▶ broadcast ordered PTY bytes
```

The browser opens a shell with `openShell`, sends keystrokes with `writeShell`,
and renders stdout/stderr in their original PTY wire order from the combined
`shellData` broadcast event (routed by `shellId`, with a small pre-subscription
buffer). A separate `shellStderr` event remains available for diagnostics but is
not rendered a second time. This mirrors the actor terminal in
`packages/shell/src/actor-vm.ts`. The VM and its shells live inside the actor's
Rust plugin, so there is no Node terminal proxy — `registry.start()` hosts the
actor and the browser talks to it directly.

## In-browser VM version

The browser-local terminal uses the production browser runtime driver and
converged Agent OS WASM sidecar. The shell is the real Brush WASM shell. Real
WASM executable bytes for Vim, Git, Bash, and the minimal core command set are
written under `/opt/agentos/pkgs/browser-terminal/0.0.1/bin` and linked through
`/opt/agentos/bin`, `/bin`, and `/usr/bin`; the process host reads the executable
selected by guest `PATH` lookup. There are no empty executable markers or
demo-specific basename dispatch. Pi is the real bundled Pi CLI attached to an
Agent OS browser PTY. A clearly labeled deterministic model adapter makes the Pi
prompt round trip reproducible without credentials or a network model.

Both pages launch upstream Brush, compiled with the generic Agent OS WASI patch
set, using Brush's built-in `minimal` interactive input backend. Brush still runs
on the Agent OS PTY; this backend avoids cursor-position queries whose
action-by-action round trip can be visible over a browser transport. Vim retains
its normal raw/full-screen PTY behavior.

No terminal bytes, filesystem operations, or process execution cross the Actor
API in this version.

## Run both versions

From the repo root:

```bash
pnpm install
pnpm --filter @rivet-dev/agentos-example-browser-terminal dev
```

or from this directory:

```bash
pnpm dev            # RivetKit server (:6420) + Vite (:5173)
```

Open http://localhost:5173 and choose a mode. In the actor version, click **+ New
VM** before opening a terminal. In the in-browser version, open **+ shell** or
**+ pi** directly.

Build a deployable static demo explicitly with `pnpm build:demo`. The ordinary
workspace `pnpm build` only runs the TypeScript gate because browser WASM asset
assembly requires `wasm-pack`; `pnpm dev`, `pnpm build:demo`, and `pnpm test:e2e`
prepare those runnable assets.

The end-to-end gate builds the browser runtime assets plus the repository's
native Agent OS sidecar and actor plugin. In both modes it drives real Vim over
the PTY to write a shell script, marks and executes that script, creates a local
Git commit, checks the commit id, and verifies Brush never emits the former
missing-child-PID warning. It also proves a Pi TUI/model turn independently in
both modes:

```bash
pnpm test:e2e
```

Run the pieces separately if you prefer:

```bash
pnpm server         # registry.start() on :6420
pnpm web            # Vite dev server on :5173
```

Override the web→server endpoint with `VITE_AGENTOS_ENDPOINT` (default
`http://localhost:6420`).

The in-browser runtime requires cross-origin isolation. This Vite configuration
sets `Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp` for both dev and preview; configure
the same headers if serving `dist/` from another static host.

The demo defaults `RIVETKIT_STORAGE_PATH` to a local ignored
`.rivetkit-data/` directory so it can run alongside RivetKit servers from other
workspaces. Set the environment variable explicitly to use a different durable
location.

## Notes

- Actor software: `@agentos-software/common` (core commands), `fd`, `ripgrep`,
  `git`, `vim`, and `@agentos-software/pi` (the Pi CLI/TUI and ACP adapter).
- Pi boots without credentials and displays `no-model` until you use `/login` or
  provide one of Pi's supported API-key environment variables.
- The shipped actor has no `listShells` action and keeps no server-side
  scrollback, so reconnect re-adopts saved shell ids and resumes **live** output
  only (history from before the reload is not replayed). Stale ids (VM recreated)
  are dropped after a liveness probe.
- xterm input is forwarded byte-for-byte to the VM PTY. Echo, line editing,
  control keys, cursor movement, and full-screen rendering are owned by the
  guest terminal stack, so interactive programs such as Pi work normally.
