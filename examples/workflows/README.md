---
title: "Workflows"
description: "Durable multi-step workflows that drive a VM across restarts, chaining each step's output into the next."
category: "Orchestration"
order: 3
---

# Workflows

Run multi-step agent work that survives crashes and restarts. Reach for this when a task has distinct stages — clone, fix, test, record — and you want each stage to be durable, retryable, and resumable rather than a single fragile call.

## How it works

A RivetKit `actor` whose `run` handler is built with `workflow()` orchestrates the steps, while a separate `agentOS` VM actor does the actual work over the client. Each `ctx.step(...)` is recorded, retried, and resumed independently: if the process crashes mid-run, replay skips completed steps and continues from where it left off. The orchestrator loops on a durable `queue`, waiting for the next request, then runs its steps in order against the VM. Output flows step-to-step through return values and the VM filesystem — the bug-fixer chains clone -> fix -> test -> record, and the code-reviewer writes a review file in one agent session and feeds it into a second. Sessions are created and closed inside a step, so they never outlive the work they back.

## Run it

```bash
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # start the orchestrator + VM
npx tsx client.ts                            # trigger the durable bug-fix workflow
```

The client sends a request to the workflow queue; the workflow drives the VM through each step and prints the last issue and test exit code.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/workflows
