# Inspector

Watch and drive a live VM from the Rivet dashboard: transcript, terminal, filesystem, processes, and system tabs.

Every agentOS actor ships a set of tabs for the Rivet dashboard's actor inspector. Open a running VM and you can read the agent's transcript, send it prompts, answer permission requests, open a shell, browse the filesystem, and watch its processes. Nothing to configure: the tabs register automatically when you use `agentOS()`.

## Open the inspector

Any server with an agentOS actor gets the inspector for free:

Start the server, open the dashboard, and click into the actor:

```bash
npx tsx server.ts
# then open http://localhost:6420/ui
```

The stock rivetkit tabs (state, connections, console) are replaced by the agentOS tabs. The tab assets are served by your server process through the actor gateway, so the same tabs work wherever the dashboard can reach the actor, locally or deployed.

## Transcript

A chat view of every session on the VM: user and agent messages, thinking, tool calls with their inputs and outputs, and the agent's plan. Plumbing events collapse into expandable rows so they never dominate the pane.

The composer at the bottom drives the agent directly: pick an agent type, set per-session env vars (such as an API key, which stays in your browser), and send prompts. This is the same data and the same actions your code uses via `createSession`, `sendPrompt`, and the `sessionEvent` stream. See [Sessions](/docs/sessions).

## Approvals

When an agent asks for permission, a banner appears above every tab with the request and three replies: approve once, always allow, or deny. The agent's turn blocks until someone answers (the runtime auto-rejects after its permission timeout), so an unanswered request is the most common reason an agent looks stuck. See [Approvals](/docs/approvals).

## Terminal

An interactive shell into the VM (`sh` by default). Starting a shell boots the VM if it is asleep, so the tab waits for an explicit click. Use it to inspect what the agent actually did: `ls` its working tree, re-run a failing command, or check the environment. See [Processes & Shell](/docs/processes).

## Filesystem

Browse the VM's filesystem and read files, the same view your code gets through the fs API. Note that the root filesystem is in-memory; files outside persisted mounts do not survive VM restarts. See [Filesystem](/docs/filesystem).

## Processes

The full kernel process tree, refreshed live. Select a process for its details (ppid, cwd, driver, exit code), stop or kill it, and watch a live output tail for processes spawned through the SDK. When the VM is asleep, the tab says so instead of waking it. See [Processes & Shell](/docs/processes).

## System

What the VM is made of: installed software bundles with their commands and versions, and the configured mounts with their access modes. See [Software](/docs/software) and [Sandbox Mounting](/docs/sandbox).

## Access control

The dashboard holds a single per-actor inspector token, and that token authorizes every action the tabs can perform, including sending prompts, answering permission requests, writing to a shell, and killing processes. There is no per-action scoping: treat dashboard access to an actor as operator access to its VM.

## How it relates to your code

The inspector is a window onto the same actor actions the SDK exposes; nothing it shows is inspector-only state. Every tab maps to an API you can call from your own code, so anything you find while inspecting (a session to resume, a file to read, a process to kill) is one SDK call away.