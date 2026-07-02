# Browser Terminal

A full terminal for Agent OS VMs that runs in the browser, talking to a
[RivetKit](https://rivetkit.org) actor over its live connection — no bespoke
WebSocket server.

- **Left sidebar** — a list of VMs. Each is one Agent OS VM (one RivetKit actor
  instance). The VM ids are kept in `localStorage`, so reopening the page — or
  clicking a VM again — reconnects to the same running VM.
- **Tabs** — each VM can have multiple terminal sessions (PTY shells).
- **Reconnect** — because the actor keeps its VM (and shells) alive, a browser
  that reconnects re-adopts the running shells and replays their scrollback.

This example is a **standalone project** (its own `node_modules`, installed from
published npm packages) so it runs without building the agent-os monorepo.

## Requirements

- `npm install` pulls everything from npm, including the prebuilt Agent OS
  sidecar and the WASM coreutils in `@agentos-software/common`.
- Hosting a RivetKit actor locally uses a local Rivet engine + actor-host envoy.
  The engine binary ships with `@rivetkit/engine-cli` (installed automatically),
  and the native registry builder that wires it up lives in the sibling
  `r6` rivetkit checkout. `npm run server` (via `run-server.mjs`) locates both;
  set `AGENTOS_R6_ROOT` if your `r6` checkout is elsewhere.

## How it works

```
Browser (React + xterm.js)              Node (RivetKit server, server.ts)
  ├─ useActor({ name: "shellVm", key })  ├─ actor "shellVm"
  ├─ openShell / writeShell / resize ───▶│    └─ vars.vm = AgentOs.create({ software:[common] })
  ├─ getShellBuffer / listShells        │    actions: openShell/writeShell/resizeShell/
  └─ useEvent("shellData" | "shellExit")◀┘             closeShell/listShells/getShellBuffer
                                             onShellData ─▶ broadcast("shellData", { shellId, data })
```

The actor is a hand-written RivetKit actor (not the shipped `agentOS()` actor)
because the browser needs an interactive PTY channel. It wraps the public
`@rivet-dev/agentos-core` shell API (`openShell`, `onShellData`, `writeShell`,
`resizeShell`, `closeShell`) and streams PTY bytes to connected browsers as
`shellData` events (base64), broadcasting `shellExit` when a shell closes. The
VM handle is a non-serializable runtime resource, so it lives in the actor's
`vars`.

## Run

```bash
npm install
npm run dev          # starts the RivetKit server (engine :6642) and Vite (:5173)
```

First boot spawns a local Rivet engine and registers the actor host, which takes
~30–40s; if the page loads before then it will retry until the host is ready.

Open http://localhost:5173, click **+ New VM**, then **+** to open a terminal
and start typing (`ls`, `echo hi | tr a-z A-Z`, `cd /tmp`, …).

Run the pieces separately if you prefer:

```bash
npm run server       # RivetKit engine + actor host on :6642
npm run web          # Vite dev server on :5173
```

Override the engine port with `PORT` and the web→engine endpoint with
`VITE_AGENTOS_ENDPOINT` (default `http://localhost:6642`).

## Notes

- Software: `@agentos-software/common` (provides `sh`) plus `everything`, `git`,
  `http-get`, and `sqlite3` — roughly the tool set the agentos shell ships
  (`curl`, `rg`, `grep`, `sed`, `jq`, `tree`, `git`, `sqlite3`, …). Agent OS has
  no vim/editor package, so there is no in-VM editor.
- Output is pulled via a `readShell(shellId, offset)` action (an incremental
  cursor), not pushed as events: in this local native-registry setup RivetKit
  only flushes broadcasts to a connection when another connection is active, so a
  lone browser driving its own shell never sees its own output over events.
- The VM shell is line-buffered (it only echoes a line on Enter), so the client
  does **local echo + line editing** (printable chars, Backspace, Ctrl-C) and
  suppresses the shell's own echo of the submitted line to avoid double display.
- Scrollback replayed on reconnect is bounded (256 KiB per shell) on the server.
