---
title: "Code Mode"
description: "Let generated code orchestrate narrow host functions in one VM process."
---

Code Mode gives an LLM one execution tool instead of exposing every host tool
directly. This example registers a Zod-validated weather host function, lets a
generated expression invoke it more than once, and returns one structured
result.

Host-function handlers run in the trusted host. Only validated input and JSON output
cross the agentOS boundary, so credentials and direct host resources stay out
of generated code.

## Run it

```bash
pnpm --dir examples/js-code-mode start
```
