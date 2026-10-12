---
title: "Host Functions"
description: "Expose host functions to a VM as typed CLI commands."
---

# Host Functions

Define a record of collections, each containing functions with a Zod `inputSchema` and an `execute` handler. Pass that record to `AgentOs.create({ hostFunctions })`; agentOS installs an `agentos-{name}` CLI for each collection and validates every invocation with its Zod schema before executing the host callback.
