---
title: "Host Functions"
description: "Expose host functions to a VM as typed CLI commands."
---

# Host Functions

Define individual functions with `hostFunction()` and group them with `hostFunctions()`. Pass the collections to `AgentOs.create({ hostFunctions })`; agentOS installs an `agentos-{name}` CLI for each collection and validates every invocation with its Zod schema before executing the host callback.
