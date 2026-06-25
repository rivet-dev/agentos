---
title: "Bash"
description: "Run shell commands, pipes, and file manipulation inside the VM with vm.exec()."
category: "Quickstart"
order: 2
---

Run real shell commands inside a VM. Reach for this when you need to shell out — invoke CLIs, chain pipes, or read and write files — without leaving the sandbox.

## How it works

Create a VM with `AgentOs.create()`, then call `vm.exec()` with any shell string. Each call runs the command in the VM and resolves to a result carrying `stdout`, `stderr`, and `exitCode`. Because it's a real shell, pipes (`|`), redirects (`>`, `>>`), and tools like `grep` and `tr` work as written, and files persist across calls within the same VM. Call `vm.dispose()` when you're done to release it.

## Run it

```sh
npm install
npm run start -- bash
```

You'll see the echoed greeting, the piped uppercase output, the contents of `/tmp/test.txt`, the matched grep line, and a final exit code of `0`.

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/quickstart/bash
