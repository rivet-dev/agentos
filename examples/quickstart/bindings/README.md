---
title: "Bindings"
description: "Expose host functions to a VM as typed CLI bindings."
---

# Bindings

Define individual bindings with `binding()` and group them with `bindings()`. Pass the collections to `AgentOs.create({ bindings })`; AgentOS installs an `agentos-{name}` CLI for each collection and validates every invocation with its Zod schema before executing the host callback.
