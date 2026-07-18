---
title: "Workflows"
description: "Durable multi-step workflows that drive a VM across restarts, chaining each step's output into the next."
category: "Orchestration"
order: 3
---

# Workflows

Run multi-step agent work that survives crashes and restarts. Reach for this when a task has distinct stages — clone, fix, test, record — and you want each stage to be durable, retryable, and resumable rather than a single fragile call.

## How it works

A RivetKit `actor` whose `run` handler is built with `workflow()` orchestrates the steps, while a separate `agentOS` VM actor does the actual work over the client. Each workflow actor instance represents one run and stores its immutable creation input in actor state. Each `ctx.step(...)` is recorded, retried, and resumed independently: if the process crashes mid-run, replay skips completed steps and continues from where it left off. Output flows step-to-step through return values and the VM filesystem — the bug-fixer chains clone -> fix -> test -> record, and the code-reviewer writes a review file and feeds it into the next step. No application queue is required; AgentOS itself serializes prompts targeting the same session.

## Run it

```bash
npm install
ANTHROPIC_API_KEY=sk-... npx tsx server.ts   # start the orchestrator + VM
npx tsx client.ts                            # trigger the durable bug-fix workflow
```

The client creates a workflow actor with input, waits for its durable status to become complete, and prints the test exit code.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/workflows
