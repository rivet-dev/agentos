---
title: "Host Functions"
description: "Expose trusted host functions to embedded VMs as Zod-typed commands."
category: "Reference"
order: 3
---

Give embedded VM programs access to trusted host code through type-safe inputs and an automatically generated CLI.

Pass `AgentOs.create({ hostFunctions })` a record of collections. Collection keys name the `/bin/agentos-{name}` commands; function keys name their subcommands. Each function defines a Zod `inputSchema` and an `execute` callback. The hosted actor does not accept host functions; they run only in a trusted embedding process.

## Run it

```sh
npm install
WEATHER_API_KEY=... npx tsx exec-bash.ts
```

The guest command calls the `weather` host function and writes its result into the VM filesystem.
