---
title: "Software"
description: "Declare which software packages and CLI commands are available inside the VM."
category: "Reference"
order: 2
---

The commands an agent can run are determined by the software you install into its VM. This example declares a software set so a shell pipeline like `echo hello | grep hello` resolves inside the sandbox.

## How it works

`agentOS({ software: [...] })` takes a list of imported software packages, and together they define the CLI surface available to the guest. Common utilities — coreutils, sed, grep, gawk, findutils, diffutils, tar, and gzip — ship by default, so you only list the extras you need; here `pi` adds the agent itself. The client then runs commands through the VM via `exec`, which only succeed when the underlying binaries are present in the declared software set.

## Run it

```sh
npm install
npm run server   # starts the registry on http://localhost:6420
npm run client   # runs "echo hello | grep hello" in the VM, prints "hello"
```

## Source

View the source on GitHub: https://github.com/rivet-dev/agent-os/tree/main/examples/software
