---
title: "Networking"
description: "Proxy HTTP requests into VM ports and create expiring preview routes."
category: "Networking"
order: 1
---

The examples use `network.fetch`, `network.fetchStream.*`, and
`network.preview.*` on the generated fixed-actor client. Preview records are
bounded, TTL-based actor SQLite state.

```bash
npm install
npx tsx client-run-server.ts
npx tsx client-fetch.ts
npx tsx client-preview.ts
```

The actor process must be available at `http://localhost:6420`.
