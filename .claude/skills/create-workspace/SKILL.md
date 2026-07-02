---
name: create-workspace
description: Create a new shared topic workspace with fresh jj workspaces for agent-os and secure-exec under ~/workspaces/{topic}/, each on a new change off latest main. Trigger ANY time the user says "create a new workspace" (or asks for a new worktree/workspace for a topic).
---

# Create a topic workspace (agent-os + secure-exec)

Creates `~/workspaces/<topic>/` with sibling jj workspaces `agentos/` and
`secure-exec/`, each with a fresh working copy on top of the freshly fetched
`main@origin`.

Derive `<topic>` as a short kebab-case slug from the user's ask (e.g.
"create a new workspace for the poll rework" → `poll-rework`). If the user
did not imply a topic, ask for one.

## Steps

```bash
topic=<topic>
mkdir -p ~/workspaces/$topic

# agent-os
cd ~/agent-os
jj git fetch
jj workspace add --name $topic ~/workspaces/$topic/agentos
cd ~/workspaces/$topic/agentos
jj new 'main@origin' -m "wip: $topic"

# secure-exec
cd ~/secure-exec
jj git fetch
jj workspace add --name $topic ~/workspaces/$topic/secure-exec
cd ~/workspaces/$topic/secure-exec
jj new 'main@origin' -m "wip: $topic"
```

If `jj workspace add` fails because the name is taken, suffix it
(`$topic-2`) — the directory path stays as requested.

## STRICT: these workspaces ARE the session's working directories

From the moment this skill runs, ALL work for the rest of the session happens
inside `~/workspaces/<topic>/agentos` and `~/workspaces/<topic>/secure-exec`.

- NEVER edit, build, or run jj/git state-changing commands in the main
  checkouts (`~/agent-os`, `~/secure-exec`) or in any other workspace for the
  remainder of the session. They belong to other sessions; their working
  copies are live shared state.
- The ONLY allowed operations against the main checkouts are read-only git
  plumbing that does not touch a working copy (e.g. `git -C ~/agent-os
  fetch`, `gh` calls, creating a temporary `git worktree` for assembling a
  PR branch).
- If a command needs a repo path, spell out the workspace path explicitly —
  do not rely on a remembered cwd.

## Notes

- The two workspaces are siblings, so agent-os local dep mode works
  unchanged: run `just secure-exec-local` inside the new `agentos/` workspace
  to point deps at `../secure-exec`.
- `dist/`, `vendor/`, `node_modules/`, registry artifacts are gitignored and
  NOT present in a fresh workspace — run `pnpm install` (both repos) and the
  builds the task needs (see the repo CLAUDE.md "Testing a local build"
  sections) before relying on them.
