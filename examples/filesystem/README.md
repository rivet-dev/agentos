---
title: "Filesystem"
description: "Read and write the actor VFS and configure whitelisted durable mounts."
category: "Filesystem"
order: 1
---

`operations.ts` uses the generated `filesystem.*` actions. The hosted actor
accepts only the fixed `actor-sqlite` backend from its filesystem registry; it
cannot mount host paths or callback-backed filesystems.

The host-directory, S3, and Google Drive examples use embedded
`@rivet-dev/agentos-core`, where the trusted embedding process may configure
host-backed mount plugins.

```bash
npm install
npx tsx operations.ts
```

The hosted actor process must be available at `http://localhost:6420`.
