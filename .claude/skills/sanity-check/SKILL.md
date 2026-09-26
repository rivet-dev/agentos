---
name: sanity-check
description: Run the deferred agentOS VM smoke test from public npm packages. Use when the user asks to sanity check, smoke test, or verify a release works.
---

# Sanity Check

This is a full-validation check. Install `@rivet-dev/agentos-core` from npm in
a fresh temporary project, boot one VM, verify filesystem and process execution,
then dispose it cleanly.

## Test program

```js
import { AgentOs } from "@rivet-dev/agentos-core";

const vm = await AgentOs.create();
try {
  await vm.filesystem.writeFile("/tmp/test.txt", "Hello from agentOS!\n");
  const result = await vm.process.exec("cat /tmp/test.txt");
  if (result.exitCode !== 0 || result.stdout !== "Hello from agentOS!\n") {
    throw new Error(JSON.stringify(result));
  }
  console.log("agentOS smoke passed");
} finally {
  await vm.dispose();
}
```

## Rules

- Always use a fresh temporary directory, never the repository.
- Install from the public npm registry without local links.
- Report the installed `@rivet-dev/agentos-core` version.
- Use Node.js 22 or newer.
- Apply a two-minute outer timeout.
- Remove the temporary directory after completion.
- Do not install registry software from npm. Core ships its default immutable
  `.aospkg` bundle.
